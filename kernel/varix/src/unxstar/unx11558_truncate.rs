//! UNX-F11558 truncate 尺寸收缩/扩展
//!
//! `truncate` 只改「长度」不改「内容」：缩短时尾部字节丢弃，扩展时新增区间必须是
//! 稀疏零洞。它对长度同时给出三条互斥的取值路径，任何一条算错都会让文件变成
//! 基准机不认的形状：
//!
//! 1. **符号前缀**：`-s 4096` 是绝对值；`+24` 是「在原长上再加」，`-24` 是
//!    「从原长里再减」。减法必须走 `checked_sub`——缩到负数是 `TR_ERANGE`，
//!    不能回绕成天文数字。
//! 2. **百分号**：`50%` 表示「按现有长度的百分比」，用 `u128` 中间量算
//!    `cur × pct / 100`，再回收成 `u64`；`pct` 本身也有上限（10000%）。
//! 3. **块口径**：`--io-blocks` 时 `SIZE` 的单位是 512 字节块，块数最后才乘；
//!    `%` 不受块口径影响（百分比没有单位）。
//!
//! 尺寸字面还有一套单位表：`K`=1024、`KB`=1000、`KiB`=1024，`M/G/T/P/E` 同理。
//! 大小写敏感——`k` 与 `K` 在基准机上是两个不同后缀，这里照抄不一致的规则：
//! 只认大写 K/M/G/T/P/E，小写一律 `TR_EBYTE`。
//!
//! 闸门次序：选项字面（`TR_EINVAL`）→ 长度（`TR_ELEN`）→ 字节（`TR_EBYTE`）→
//! 数值越界（`TR_ERANGE`）→ 重复（`TR_EDUP`）→ 尺寸缺失（`TR_ESIZE`）→
//! 引用不存在（`TR_ENOENT`）→ 组合冲突（`TR_ESTATE`）。
//!
//! 账本语义：十四格 = 八格成功（绝对值、增量、加长、缩短、百分比、块取整、
//! 零洞跨度、引用长度）+ 六格失败（坏字节、空值、缩到负、双符号、重复选项、
//! 组合冲突）。`audit` 认「八值六码」，`seal` 做 FNV-1a 折叠，`replay` 重建
//! 比对，`freeze` 后拒绝登记。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf3f0;

/// 选项字面非法。
pub const TR_EINVAL: u32 = SEG_BASE + 0x01;
/// 选项或路径过长。
pub const TR_ELEN: u32 = SEG_BASE + 0x02;
/// 字节不合法。
pub const TR_EBYTE: u32 = SEG_BASE + 0x03;
/// 数值越界。
pub const TR_ERANGE: u32 = SEG_BASE + 0x04;
/// 选项重复。
pub const TR_EDUP: u32 = SEG_BASE + 0x05;
/// 尺寸缺失。
pub const TR_ESIZE: u32 = SEG_BASE + 0x06;
/// 引用不存在。
pub const TR_ENOENT: u32 = SEG_BASE + 0x07;
/// 组合冲突。
pub const TR_ESTATE: u32 = SEG_BASE + 0x08;
/// 账本失衡。
pub const TR_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位错位。
pub const TR_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const TR_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const TR_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观测缺失。
pub const TR_ENOBS: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 尺寸字面长度上限。
pub const SIZECAP: usize = 24;

/// 路径容量。
pub const PATHCAP: usize = 64;

/// 引用表容量。
pub const REFCAP: usize = 8;

/// 参数个数上限。
pub const ARGCAP: usize = 16;

/// 百分比上限。
pub const PCTMAX: u64 = 10_000;

/// 块大小（POSIX 512）。
pub const BLOCK: u64 = 512;

/// 单个文件尺寸上限（1 EiB 以下留出保护区）。
pub const SIZEMAX: u64 = 1u64 << 60;

/// 操作码：解析。
pub const OP_PARSE: u8 = 0x01;
/// 操作码：计划。
pub const OP_PLAN: u8 = 0x02;
/// 操作码：块取整。
pub const OP_BLOCK: u8 = 0x03;
/// 操作码：零洞。
pub const OP_FILL: u8 = 0x04;
/// 操作码：引用。
pub const OP_REF: u8 = 0x05;
/// 操作码：选项。
pub const OP_OPT: u8 = 0x06;

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
        if let Some(d) = buf.get_mut(i) {
            *d = b'0' + (v % 10) as u8;
        }
        v /= 10;
    }
    let mut k = i;
    while k < 20 {
        if let Some(d) = buf.get(k) {
            s.push(*d as char);
        }
        k += 1;
    }
}

/// 十进制文本。
fn dec_text(v: u64) -> String {
    let mut s = String::new();
    push_dec(&mut s, v);
    s
}

/// 尺寸单位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    /// 字节。
    Byte,
    /// 1024 进位（`K`）。
    Ki,
    /// 1000 进位（`KB`）。
    Kb,
    /// 1024²（`M`）。
    Mi,
    /// 1000²（`MB`）。
    Mb,
    /// 1024³（`G`）。
    Gi,
    /// 1000³（`GB`）。
    Gb,
    /// 1024⁴（`T`）。
    Ti,
    /// 1000⁴（`TB`）。
    Tb,
    /// 1024⁵（`P`）。
    Pi,
    /// 1000⁵（`PB`）。
    Pb,
    /// 1024⁶（`E`）。
    Ei,
    /// 1000⁶（`EB`）。
    Eb,
}

impl Unit {
    /// 乘数；超出 `u64` 时给 `None`。
    pub fn factor(&self) -> Option<u64> {
        let (base, exp) = match self {
            Unit::Byte => (1u64, 0u8),
            Unit::Ki => (1024, 1),
            Unit::Kb => (1000, 1),
            Unit::Mi => (1024, 2),
            Unit::Mb => (1000, 2),
            Unit::Gi => (1024, 3),
            Unit::Gb => (1000, 3),
            Unit::Ti => (1024, 4),
            Unit::Tb => (1000, 4),
            Unit::Pi => (1024, 5),
            Unit::Pb => (1000, 5),
            Unit::Ei => (1024, 6),
            Unit::Eb => (1000, 6),
        };
        let mut v = 1u64;
        let mut i = 0u8;
        while i < exp {
            v = v.checked_mul(base)?;
            i += 1;
        }
        Some(v)
    }

    /// 名称。
    pub fn text(&self) -> &'static str {
        match self {
            Unit::Byte => "",
            Unit::Ki => "K",
            Unit::Kb => "KB",
            Unit::Mi => "M",
            Unit::Mb => "MB",
            Unit::Gi => "G",
            Unit::Gb => "GB",
            Unit::Ti => "T",
            Unit::Tb => "TB",
            Unit::Pi => "P",
            Unit::Pb => "PB",
            Unit::Ei => "E",
            Unit::Eb => "EB",
        }
    }

    /// 是否 1024 口径。
    pub fn binary(&self) -> bool {
        match self {
            Unit::Byte | Unit::Kb | Unit::Mb | Unit::Gb | Unit::Tb | Unit::Pb | Unit::Eb => false,
            _ => true,
        }
    }
}

/// 尺寸作用方式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// 绝对值。
    Set,
    /// 加长。
    Grow,
    /// 缩短。
    Shrink,
    /// 百分比。
    Percent,
}

impl Mode {
    /// 名称。
    pub fn text(&self) -> &'static str {
        match self {
            Mode::Set => "set",
            Mode::Grow => "grow",
            Mode::Shrink => "shrink",
            Mode::Percent => "percent",
        }
    }

    /// 是否有符号。
    pub fn signed(&self) -> bool {
        *self == Mode::Grow || *self == Mode::Shrink
    }
}

/// 解析好的尺寸字面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SizeSpec {
    /// 作用方式。
    pub mode: Mode,
    /// 字节数（`Percent` 时是百分数）。
    pub bytes: u64,
    /// 单位（`Percent` 时为 `Unit::Byte`）。
    pub unit: Unit,
    /// 是否显式给了单位。
    pub explicit: bool,
}

impl SizeSpec {
    /// 绝对值字节。
    pub fn absolute(v: u64) -> SizeSpec {
        SizeSpec { mode: Mode::Set, bytes: v, unit: Unit::Byte, explicit: false }
    }

    /// 解析：`[+|-]<数字>[单位]` 或 `<数字>%`。
    pub fn parse(raw: &[u8]) -> Result<SizeSpec, u32> {
        if raw.is_empty() {
            return Err(TR_EINVAL);
        }
        if raw.len() > SIZECAP {
            return Err(TR_ELEN);
        }
        let mut i = 0usize;
        let mut mode = Mode::Set;
        match raw.first() {
            Some(b'+') => {
                mode = Mode::Grow;
                i = 1;
            }
            Some(b'-') => {
                mode = Mode::Shrink;
                i = 1;
            }
            _ => {}
        }
        let mut v = 0u64;
        let mut digits = 0u32;
        while i < raw.len() {
            let c = match raw.get(i) {
                Some(x) => *x,
                None => break,
            };
            if c < b'0' || c > b'9' {
                break;
            }
            v = match v.checked_mul(10) {
                Some(x) => x,
                None => return Err(TR_ERANGE),
            };
            v = match v.checked_add((c - b'0') as u64) {
                Some(x) => x,
                None => return Err(TR_ERANGE),
            };
            digits += 1;
            i += 1;
        }
        if digits == 0 {
            return Err(TR_EINVAL);
        }
        let rest = raw.get(i..).unwrap_or(&[]);
        if rest.is_empty() {
            return Ok(SizeSpec { mode, bytes: v, unit: Unit::Byte, explicit: false });
        }
        if rest.len() == 1 {
            if let Some(b'%') = rest.first() {
                if mode != Mode::Set {
                    return Err(TR_EBYTE);
                }
                if v > PCTMAX {
                    return Err(TR_ERANGE);
                }
                return Ok(SizeSpec { mode: Mode::Percent, bytes: v, unit: Unit::Byte, explicit: true });
            }
        }
        let unit = match rest {
            b"K" => Unit::Ki,
            b"KB" => Unit::Kb,
            b"KiB" => Unit::Ki,
            b"M" => Unit::Mi,
            b"MB" => Unit::Mb,
            b"MiB" => Unit::Mi,
            b"G" => Unit::Gi,
            b"GB" => Unit::Gb,
            b"GiB" => Unit::Gi,
            b"T" => Unit::Ti,
            b"TB" => Unit::Tb,
            b"TiB" => Unit::Ti,
            b"P" => Unit::Pi,
            b"PB" => Unit::Pb,
            b"PiB" => Unit::Pi,
            b"E" => Unit::Ei,
            b"EB" => Unit::Eb,
            b"EiB" => Unit::Ei,
            b"B" => Unit::Byte,
            _ => return Err(TR_EBYTE),
        };
        let factor = match unit.factor() {
            Some(f) => f,
            None => return Err(TR_ERANGE),
        };
        let bytes = match v.checked_mul(factor) {
            Some(x) => x,
            None => return Err(TR_ERANGE),
        };
        if bytes > SIZEMAX {
            return Err(TR_ERANGE);
        }
        Ok(SizeSpec { mode, bytes, unit, explicit: true })
    }

    /// 文本解析。
    pub fn parse_text(text: &str) -> Result<SizeSpec, u32> {
        SizeSpec::parse(text.as_bytes())
    }

    /// 套块口径：非百分比时乘 512。
    pub fn with_blocks(&self) -> Result<SizeSpec, u32> {
        if self.mode == Mode::Percent {
            return Ok(*self);
        }
        let bytes = match self.bytes.checked_mul(BLOCK) {
            Some(v) => v,
            None => return Err(TR_ERANGE),
        };
        if bytes > SIZEMAX {
            return Err(TR_ERANGE);
        }
        Ok(SizeSpec { mode: self.mode, bytes, unit: self.unit, explicit: self.explicit })
    }

    /// 文本（显式单位能整除时按原单位回写，否则退回字节）。
    pub fn text(&self) -> String {
        let mut s = String::new();
        if self.mode == Mode::Grow {
            s.push('+');
        } else if self.mode == Mode::Shrink {
            s.push('-');
        }
        if self.mode == Mode::Percent {
            push_dec(&mut s, self.bytes);
            s.push('%');
            return s;
        }
        let mut mant = self.bytes;
        let mut unit = Unit::Byte;
        if self.explicit {
            if let Some(f) = self.unit.factor() {
                if f > 1 && self.bytes % f == 0 {
                    mant = self.bytes / f;
                    unit = self.unit;
                }
            }
        }
        push_dec(&mut s, mant);
        s.push_str(unit.text());
        s
    }
}

/// 计算目标长度。
pub fn plan(cur: u64, spec: &SizeSpec) -> Result<u64, u32> {
    let next = match spec.mode {
        Mode::Set => spec.bytes,
        Mode::Grow => match cur.checked_add(spec.bytes) {
            Some(v) => v,
            None => return Err(TR_ERANGE),
        },
        Mode::Shrink => match cur.checked_sub(spec.bytes) {
            Some(v) => v,
            None => return Err(TR_ERANGE),
        },
        Mode::Percent => {
            let p = spec.bytes as u128;
            let c = cur as u128;
            let v = match c.checked_mul(p) {
                Some(x) => x,
                None => return Err(TR_ERANGE),
            } / 100;
            match u64::try_from(v) {
                Ok(x) => x,
                Err(_) => return Err(TR_ERANGE),
            }
        }
    };
    if next > SIZEMAX {
        return Err(TR_ERANGE);
    }
    Ok(next)
}

/// 块数向上取整。
pub fn blocks(v: u64) -> u64 {
    v.saturating_add(BLOCK - 1) / BLOCK
}

/// 零洞跨度：`cur < next` 时返回 `(start, len)`。
pub fn fill_span(cur: u64, next: u64) -> Option<(u64, u64)> {
    if next <= cur {
        return None;
    }
    Some((cur, next - cur))
}

/// 动作计划。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TruncPlan {
    /// 原长。
    pub from: u64,
    /// 新长。
    pub to: u64,
    /// 零洞起点（无扩展时等于新长）。
    pub fill_start: u64,
    /// 零洞长度。
    pub fill_len: u64,
    /// 新长块数。
    pub blocks: u64,
    /// 是否必须建文件。
    pub creates: bool,
}

/// 生成计划；`no_create` 且文件不存在给 `TR_ENOENT`。
pub fn plan_file(cur: u64, spec: &SizeSpec, no_create: bool, exists: bool) -> Result<TruncPlan, u32> {
    if no_create && !exists {
        return Err(TR_ENOENT);
    }
    let to = plan(cur, spec)?;
    let (fill_start, fill_len) = match fill_span(cur, to) {
        Some((s, l)) => (s, l),
        None => (to, 0),
    };
    Ok(TruncPlan {
        from: cur,
        to,
        fill_start,
        fill_len,
        blocks: blocks(to),
        creates: !exists,
    })
}

/// 引用文件表：名字 → 长度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refs {
    rows: [Option<(Name, u64)>; REFCAP],
    len: usize,
}

impl Refs {
    /// 空表。
    pub const fn new() -> Refs {
        Refs { rows: [None; REFCAP], len: 0 }
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 登记；同名覆盖，满表给 `TR_ELEN`。
    pub fn add(&mut self, name: Name, size: u64) -> Result<(), u32> {
        let mut i = 0usize;
        while i < self.len {
            let hit = match self.rows.get(i) {
                Some(Some((n, _))) => n.eq_bytes(name.as_bytes()),
                _ => false,
            };
            if hit {
                if let Some(dst) = self.rows.get_mut(i) {
                    *dst = Some((name, size));
                }
                return Ok(());
            }
            i += 1;
        }
        if self.len >= REFCAP {
            return Err(TR_ELEN);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = Some((name, size));
        }
        self.len += 1;
        Ok(())
    }

    /// 查长度。
    pub fn find(&self, name: &[u8]) -> Option<u64> {
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(Some((n, s))) => {
                    if n.eq_bytes(name) {
                        return Some(*s);
                    }
                }
                _ => {}
            }
            i += 1;
        }
        None
    }
}

/// 路径名（允许 `/`，不许控制字符）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Name {
    buf: [u8; PATHCAP],
    len: u8,
}

impl Name {
    /// 空名。
    pub const fn empty() -> Name {
        Name { buf: [0; PATHCAP], len: 0 }
    }

    /// 解析：非空、≤ `PATHCAP`、禁止 NUL/控制字符。
    pub fn parse(raw: &[u8]) -> Result<Name, u32> {
        if raw.is_empty() {
            return Err(TR_EINVAL);
        }
        if raw.len() > PATHCAP {
            return Err(TR_ELEN);
        }
        let mut buf = [0u8; PATHCAP];
        let mut i = 0usize;
        while i < raw.len() {
            match raw.get(i) {
                Some(b) => {
                    let c = *b;
                    if c < 0x20 || c == 0x7f {
                        return Err(TR_EBYTE);
                    }
                    if let Some(d) = buf.get_mut(i) {
                        *d = c;
                    }
                }
                None => return Err(TR_EBYTE),
            }
            i += 1;
        }
        Ok(Name { buf, len: raw.len() as u8 })
    }

    /// 文本解析。
    pub fn parse_text(text: &str) -> Result<Name, u32> {
        Name::parse(text.as_bytes())
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 字节视图。
    pub fn as_bytes(&self) -> &[u8] {
        self.buf.get(..self.len as usize).unwrap_or(&[])
    }

    /// 字节相等。
    pub fn eq_bytes(&self, other: &[u8]) -> bool {
        self.as_bytes() == other
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(c) = self.buf.get(i) {
                s.push(*c as char);
            }
            i += 1;
        }
        s
    }
}

/// 选项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opts {
    /// 尺寸。
    pub size: Option<SizeSpec>,
    /// 引用文件。
    pub reference: Option<Name>,
    /// 块口径。
    pub io_blocks: bool,
    /// 不建文件。
    pub no_create: bool,
    /// 参数个数。
    pub argc: usize,
}

impl Opts {
    /// 空选项。
    pub const fn blank() -> Opts {
        Opts { size: None, reference: None, io_blocks: false, no_create: false, argc: 0 }
    }

    /// 生效尺寸：`-s` 优先且两者互斥。
    pub fn effective(&self) -> Result<SizeSpec, u32> {
        match &self.size {
            Some(s) => {
                if self.io_blocks {
                    s.with_blocks()
                } else {
                    Ok(*s)
                }
            }
            None => Err(TR_ESIZE),
        }
    }
}

/// 解析选项。
pub fn parse_opts(args: &[&str]) -> Result<Opts, u32> {
    let mut o = Opts::blank();
    let mut seen_size = false;
    let mut seen_ref = false;
    let mut seen_blocks = false;
    let mut seen_create = false;
    let mut i = 0usize;
    while i < args.len() {
        if i >= ARGCAP {
            return Err(TR_ELEN);
        }
        let a = match args.get(i) {
            Some(v) => *v,
            None => break,
        };
        if a.len() > 128 {
            return Err(TR_ELEN);
        }
        if let Some(rest) = a.strip_prefix("--size=") {
            if seen_size {
                return Err(TR_EDUP);
            }
            seen_size = true;
            o.size = Some(SizeSpec::parse_text(rest)?);
        } else if a == "-s" || a == "--size" {
            if seen_size {
                return Err(TR_EDUP);
            }
            seen_size = true;
            let v = match args.get(i + 1) {
                Some(v) => *v,
                None => return Err(TR_ESIZE),
            };
            o.size = Some(SizeSpec::parse_text(v)?);
            i += 1;
        } else if let Some(rest) = a.strip_prefix("--reference=") {
            if seen_ref {
                return Err(TR_EDUP);
            }
            seen_ref = true;
            o.reference = Some(Name::parse_text(rest)?);
        } else if a == "-r" || a == "--reference" {
            if seen_ref {
                return Err(TR_EDUP);
            }
            seen_ref = true;
            let v = match args.get(i + 1) {
                Some(v) => *v,
                None => return Err(TR_EINVAL),
            };
            o.reference = Some(Name::parse_text(v)?);
            i += 1;
        } else if a == "-o" || a == "--io-blocks" {
            if seen_blocks {
                return Err(TR_EDUP);
            }
            seen_blocks = true;
            o.io_blocks = true;
        } else if a == "-c" || a == "--no-create" {
            if seen_create {
                return Err(TR_EDUP);
            }
            seen_create = true;
            o.no_create = true;
        } else {
            return Err(TR_EINVAL);
        }
        o.argc += 1;
        i += 1;
    }
    if o.size.is_some() && o.reference.is_some() {
        return Err(TR_ESTATE);
    }
    if o.size.is_none() && o.reference.is_none() {
        return Err(TR_ESIZE);
    }
    Ok(o)
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
            return Err(TR_ESLOT);
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

    /// 对账：八成功值 + 六失败码。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(TR_EBAL);
        }
        const VALS: [u32; 8] = [10 * 1024 * 1024, 1, 4096, 1024, 976, 500, 2, 3096];
        const CODES: [u32; 6] = [TR_EBYTE, TR_EINVAL, TR_ERANGE, TR_EBYTE, TR_EDUP, TR_ESTATE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(TR_EBAL),
            };
            if s.slot as usize != i {
                return Err(TR_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(TR_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(TR_EVALUE);
                        }
                    }
                    None => return Err(TR_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(TR_EVALUE);
                        }
                    }
                    None => return Err(TR_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(TR_EBAL);
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
            return Err(TR_EBAL);
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
            Err(TR_ESEAL)
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

/// 样本引用表。
pub fn sample_refs() -> Refs {
    let mut r = Refs::new();
    let a = Name::parse_text("base.img").unwrap_or(Name::empty());
    let b = Name::parse_text("blob.bin").unwrap_or(Name::empty());
    let _ = r.add(a, 65536);
    let _ = r.add(b, 1536);
    r
}

/// 构建账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(word_cell(0, OP_PARSE, {
        SizeSpec::parse(b"10M").map(|s| {
            if s.bytes > u32::MAX as u64 {
                u32::MAX
            } else {
                s.bytes as u32
            }
        })
    }));

    let _ = l.register(word_cell(1, OP_PARSE, {
        SizeSpec::parse(b"+4K").map(|s| if s.mode == Mode::Grow { 1 } else { 0 })
    }));

    let _ = l.register(word_cell(2, OP_PLAN, {
        match SizeSpec::parse(b"4096") {
            Ok(s) => plan(0, &s).map(|v| v as u32),
            Err(c) => Err(c),
        }
    }));

    let _ = l.register(word_cell(3, OP_PLAN, {
        match SizeSpec::parse(b"+24") {
            Ok(s) => plan(1000, &s).map(|v| v as u32),
            Err(c) => Err(c),
        }
    }));

    let _ = l.register(word_cell(4, OP_PLAN, {
        match SizeSpec::parse(b"-24") {
            Ok(s) => plan(1000, &s).map(|v| v as u32),
            Err(c) => Err(c),
        }
    }));

    let _ = l.register(word_cell(5, OP_PLAN, {
        match SizeSpec::parse(b"50%") {
            Ok(s) => plan(1000, &s).map(|v| v as u32),
            Err(c) => Err(c),
        }
    }));

    let _ = l.register(word_cell(6, OP_BLOCK, Ok(blocks(1000) as u32)));

    let _ = l.register(word_cell(7, OP_FILL, {
        match SizeSpec::parse(b"4096") {
            Ok(s) => plan(1000, &s).map(|v| match fill_span(1000, v) {
                Some((_, l)) => l as u32,
                None => 0,
            }),
            Err(c) => Err(c),
        }
    }));

    let _ = l.register(word_cell(8, OP_PARSE, SizeSpec::parse(b"10Q").map(|s| s.bytes as u32)));

    let _ = l.register(word_cell(9, OP_PARSE, SizeSpec::parse(b"").map(|s| s.bytes as u32)));

    let _ = l.register(word_cell(10, OP_PLAN, {
        match SizeSpec::parse(b"-20") {
            Ok(s) => plan(10, &s).map(|v| v as u32),
            Err(c) => Err(c),
        }
    }));

    let _ = l.register(word_cell(11, OP_PARSE, SizeSpec::parse(b"5+").map(|s| s.bytes as u32)));

    let _ = l.register(word_cell(12, OP_OPT, {
        parse_opts(&["-s", "10", "-s", "20"]).map(|o| o.argc as u32)
    }));

    let _ = l.register(word_cell(13, OP_OPT, {
        parse_opts(&["-s", "10", "-r", "base.img"]).map(|o| o.argc as u32)
    }));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    out[0] = {
        let a = SizeSpec::parse(b"10M").map(|s| (s.bytes, s.unit)) == Ok((10485760, Unit::Mi));
        let b = SizeSpec::parse(b"10MB").map(|s| s.bytes) == Ok(10000000);
        let c = SizeSpec::parse(b"+4K").map(|s| (s.mode, s.bytes)) == Ok((Mode::Grow, 4096));
        let d = SizeSpec::parse(b"50%").map(|s| (s.mode, s.bytes)) == Ok((Mode::Percent, 50));
        a && b && c && d
    };

    out[1] = {
        let s = match SizeSpec::parse(b"4096") {
            Ok(v) => v,
            Err(_) => SizeSpec::absolute(0),
        };
        plan(0, &s) == Ok(4096) && plan(1000, &s) == Ok(4096)
    };

    out[2] = {
        let a = match SizeSpec::parse(b"-24") {
            Ok(s) => plan(1000, &s),
            Err(_) => Err(TR_EINVAL),
        };
        let b = match SizeSpec::parse(b"-2000") {
            Ok(s) => plan(1000, &s),
            Err(_) => Err(TR_EINVAL),
        };
        a == Ok(976) && b == Err(TR_ERANGE)
    };

    out[3] = {
        let r = sample_refs();
        let found = r.find(b"blob.bin") == Some(1536);
        let missing = r.find(b"nope") == None;
        let p = match plan_file(1536, &SizeSpec { mode: Mode::Grow, bytes: 10, unit: Unit::Byte, explicit: false }, true, true) {
            Ok(v) => v.to == 1546 && v.fill_len == 10 && v.blocks == 4,
            Err(_) => false,
        };
        found && missing && p
    };

    out
}

/// 判据集合。
pub fn run_unx11558_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11558.truncate");

    s.add(
        "TR01-尺寸字面",
        {
            let a = SizeSpec::parse(b"0").map(|v| (v.mode, v.bytes)) == Ok((Mode::Set, 0));
            let b = SizeSpec::parse(b"4096").map(|v| v.bytes) == Ok(4096);
            let c = SizeSpec::parse(b"+24").map(|v| (v.mode, v.bytes)) == Ok((Mode::Grow, 24));
            let d = SizeSpec::parse(b"-24").map(|v| (v.mode, v.bytes)) == Ok((Mode::Shrink, 24));
            let e = SizeSpec::parse(b"50%").map(|v| (v.mode, v.bytes)) == Ok((Mode::Percent, 50));
            let f = SizeSpec::parse(b"+-5") == Err(TR_EINVAL);
            let g = SizeSpec::parse(b"+50%") == Err(TR_EBYTE);
            a && b && c && d && e && f && g
        },
        "static",
    );

    s.add(
        "TR02-单位表",
        {
            let a = SizeSpec::parse(b"1K").map(|v| v.bytes) == Ok(1024);
            let b = SizeSpec::parse(b"1KB").map(|v| v.bytes) == Ok(1000);
            let c = SizeSpec::parse(b"1KiB").map(|v| v.bytes) == Ok(1024);
            let d = SizeSpec::parse(b"2M").map(|v| v.bytes) == Ok(2 * 1024 * 1024);
            let e = SizeSpec::parse(b"2MB").map(|v| v.bytes) == Ok(2_000_000);
            let f = SizeSpec::parse(b"3G").map(|v| v.bytes) == Ok(3 * 1024 * 1024 * 1024);
            let g = SizeSpec::parse(b"1T").map(|v| v.bytes) == Ok(1u64 << 40);
            let h = SizeSpec::parse(b"1k") == Err(TR_EBYTE);
            let i = SizeSpec::parse(b"1ZB") == Err(TR_EBYTE);
            a && b && c && d && e && f && g && h && i
        },
        "static",
    );

    s.add(
        "TR03-三向计划",
        {
            let set = match SizeSpec::parse(b"4096") {
                Ok(v) => v,
                Err(_) => SizeSpec::absolute(0),
            };
            let grow = match SizeSpec::parse(b"+24") {
                Ok(v) => v,
                Err(_) => SizeSpec::absolute(0),
            };
            let shrink = match SizeSpec::parse(b"-24") {
                Ok(v) => v,
                Err(_) => SizeSpec::absolute(0),
            };
            plan(0, &set) == Ok(4096)
                && plan(777, &set) == Ok(4096)
                && plan(1000, &grow) == Ok(1024)
                && plan(1000, &shrink) == Ok(976)
                && plan(10, &shrink) == Err(TR_ERANGE)
                && plan(0, &shrink) == Err(TR_ERANGE)
        },
        "static",
    );

    s.add(
        "TR04-百分比",
        {
            let p50 = match SizeSpec::parse(b"50%") {
                Ok(v) => v,
                Err(_) => SizeSpec::absolute(0),
            };
            let p150 = match SizeSpec::parse(b"150%") {
                Ok(v) => v,
                Err(_) => SizeSpec::absolute(0),
            };
            let p0 = match SizeSpec::parse(b"0%") {
                Ok(v) => v,
                Err(_) => SizeSpec::absolute(0),
            };
            plan(1000, &p50) == Ok(500)
                && plan(999, &p50) == Ok(499)
                && plan(1000, &p150) == Ok(1500)
                && plan(1000, &p0) == Ok(0)
                && SizeSpec::parse(b"10001%") == Err(TR_ERANGE)
        },
        "static",
    );

    s.add(
        "TR05-块口径",
        {
            let spec = match SizeSpec::parse(b"2") {
                Ok(v) => v,
                Err(_) => SizeSpec::absolute(0),
            };
            let blockspec = spec.with_blocks();
            let a = blockspec == Ok(SizeSpec { mode: Mode::Set, bytes: 1024, unit: Unit::Byte, explicit: false });
            let b = match &blockspec {
                Ok(v) => plan(0, v) == Ok(1024),
                Err(_) => false,
            };
            let pct = match SizeSpec::parse(b"50%") {
                Ok(v) => v,
                Err(_) => SizeSpec::absolute(0),
            };
            let c = pct.with_blocks().map(|v| v.bytes) == Ok(50);
            let d = blocks(0) == 0 && blocks(1) == 1 && blocks(512) == 1 && blocks(513) == 2;
            a && b && c && d
        },
        "static",
    );

    s.add(
        "TR06-零洞与计划",
        {
            let set = match SizeSpec::parse(b"4096") {
                Ok(v) => v,
                Err(_) => SizeSpec::absolute(0),
            };
            let a = plan_file(1000, &set, false, true) == Ok(TruncPlan {
                from: 1000,
                to: 4096,
                fill_start: 1000,
                fill_len: 3096,
                blocks: 8,
                creates: false,
            });
            let b = plan_file(4096, &set, false, false) == Ok(TruncPlan {
                from: 4096,
                to: 4096,
                fill_start: 4096,
                fill_len: 0,
                blocks: 8,
                creates: true,
            });
            let c = plan_file(1000, &set, true, false) == Err(TR_ENOENT);
            let d = fill_span(10, 5).is_none() && fill_span(5, 10) == Some((5, 5));
            a && b && c && d
        },
        "static",
    );

    s.add(
        "TR07-引用表",
        {
            let mut r = Refs::new();
            let a = Name::parse_text("base.img").unwrap_or(Name::empty());
            let b = Name::parse_text("blob.bin").unwrap_or(Name::empty());
            let c = r.add(a, 65536).is_ok() && r.add(b, 1536).is_ok() && r.len() == 2;
            let d = r.find(b"blob.bin") == Some(1536) && r.find(b"nope").is_none();
            let e = r.add(b, 2048).is_ok() && r.len() == 2 && r.find(b"blob.bin") == Some(2048);
            let f = Name::parse(b"a\0b") == Err(TR_EBYTE);
            c && d && e && f
        },
        "static",
    );

    s.add(
        "TR08-缺尺寸与新建闸门",
        {
            let a = parse_opts(&[]) == Err(TR_ESIZE);
            let b = parse_opts(&["-o"]) == Err(TR_ESIZE);
            let c = parse_opts(&["-c"]).is_err();
            let d = match parse_opts(&["-s", "10", "-c"]) {
                Ok(o) => o.no_create && !o.io_blocks && o.effective() == Ok(SizeSpec::absolute(10)),
                Err(_) => false,
            };
            let e = parse_opts(&["-s"]).and_then(|o| o.effective()).is_err();
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "TR09-选项矩阵",
        {
            let a = match parse_opts(&["--size=1K", "-c"]) {
                Ok(o) => o.size.map(|s| s.bytes) == Some(1024) && o.no_create && o.argc == 2,
                Err(_) => false,
            };
            let b = match parse_opts(&["-r", "base.img"]) {
                Ok(o) => o.reference.map(|n| n.text()) == Some(String::from("base.img")) && o.size.is_none(),
                Err(_) => false,
            };
            let c = parse_opts(&["-s", "1", "-r", "x"]) == Err(TR_ESTATE);
            let d = parse_opts(&["-s", "1", "-s", "2"]) == Err(TR_EDUP);
            let e = parse_opts(&["-o", "-o"]) == Err(TR_EDUP);
            let f = parse_opts(&["-x"]) == Err(TR_EINVAL);
            a && b && c && d && e && f
        },
        "static",
    );

    s.add(
        "TR10-闸门次序",
        {
            let a = SizeSpec::parse(b"") == Err(TR_EINVAL);
            let b = SizeSpec::parse(b"+") == Err(TR_EINVAL);
            let c = SizeSpec::parse(b"10Q") == Err(TR_EBYTE);
            let d = SizeSpec::parse(b"9999999999999999999999999") == Err(TR_ELEN);
            let e = SizeSpec::parse(b"99999999999999999999") == Err(TR_ERANGE);
            let f = SizeSpec::parse(b"99999999999999999999K").map(|v| v.bytes) == Err(TR_ERANGE);
            a && b && c && d && e && f
        },
        "static",
    );

    s.add(
        "TR11-上界与文本",
        {
            let a = SizeSpec::parse(b"1E").map(|v| v.bytes) == Ok(1u64 << 60);
            let b = SizeSpec::parse(b"2E") == Err(TR_ERANGE);
            let c = SizeSpec::absolute(1024).text() == String::from("1024");
            let d = match SizeSpec::parse(b"+4K") {
                Ok(v) => v.text() == String::from("+4K"),
                Err(_) => false,
            };
            let e = match SizeSpec::parse(b"50%") {
                Ok(v) => v.text() == String::from("50%"),
                Err(_) => false,
            };
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "TR12-账本对账与重放",
        {
            let l = build_ledger();
            l.audit().is_ok()
                && l.replay().is_ok()
                && l.len() == SAMPLES
                && l.ok_count() == 8
                && l.err_count() == 6
                && l.seal() != 0
        },
        "static",
    );

    s.add(
        "TR13-冻结重放四注入",
        {
            let mut l = build_ledger();
            let frozen = l.freeze().is_ok();
            let rejected = l.register(Sample::blank()).is_err() && l.rejects() == 1;
            let root = l.root() != 0 && l.is_frozen();
            let inj = inject_four();
            frozen && rejected && root && inj[0] && inj[1] && inj[2] && inj[3]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_grammar() {
        assert_eq!(SizeSpec::parse(b"12").map(|s| s.bytes), Ok(12));
        assert_eq!(SizeSpec::parse(b"-12").map(|s| s.mode), Ok(Mode::Shrink));
        assert_eq!(SizeSpec::parse(b"+12").map(|s| s.mode), Ok(Mode::Grow));
        assert_eq!(SizeSpec::parse(b"12%").map(|s| s.mode), Ok(Mode::Percent));
        assert_eq!(SizeSpec::parse(b"12%%"), Err(TR_EBYTE));
        assert_eq!(SizeSpec::parse(b"%"), Err(TR_EINVAL));
    }

    #[test]
    fn units() {
        assert_eq!(SizeSpec::parse(b"1B").map(|s| s.bytes), Ok(1));
        assert_eq!(SizeSpec::parse(b"1E").map(|s| s.bytes), Ok(1u64 << 60));
        assert_eq!(SizeSpec::parse(b"1EB").map(|s| s.bytes), Ok(1_000_000_000_000_000_000));
        assert_eq!(SizeSpec::parse(b"2P").map(|s| s.bytes), Ok(2u64 << 50));
        assert_eq!(SizeSpec::parse(b"1g"), Err(TR_EBYTE));
    }

    #[test]
    fn plan_modes() {
        let grow = SizeSpec { mode: Mode::Grow, bytes: 512, unit: Unit::Byte, explicit: false };
        let shrink = SizeSpec { mode: Mode::Shrink, bytes: 512, unit: Unit::Byte, explicit: false };
        assert_eq!(plan(1024, &grow), Ok(1536));
        assert_eq!(plan(1024, &shrink), Ok(512));
        assert_eq!(plan(100, &shrink), Err(TR_ERANGE));
        assert_eq!(plan(u64::MAX, &grow), Err(TR_ERANGE));
    }

    #[test]
    fn percent_values() {
        let p = SizeSpec { mode: Mode::Percent, bytes: 25, unit: Unit::Byte, explicit: true };
        assert_eq!(plan(1000, &p), Ok(250));
        assert_eq!(plan(3, &p), Ok(0));
        assert_eq!(plan(0, &p), Ok(0));
    }

    #[test]
    fn block_accounting() {
        assert_eq!(blocks(512), 1);
        assert_eq!(blocks(1024), 2);
        assert_eq!(blocks(1025), 3);
        assert_eq!(blocks(0), 0);
        assert_eq!(fill_span(0, 1), Some((0, 1)));
        assert_eq!(fill_span(1, 1), None);
        assert_eq!(fill_span(9, 3), None);
    }

    #[test]
    fn plan_file_guards() {
        let s = SizeSpec::absolute(2048);
        assert_eq!(plan_file(0, &s, true, false), Err(TR_ENOENT));
        let p = plan_file(0, &s, false, false).unwrap_or(TruncPlan {
            from: 0,
            to: 0,
            fill_start: 0,
            fill_len: 0,
            blocks: 0,
            creates: false,
        });
        assert_eq!(p.to, 2048);
        assert_eq!(p.fill_len, 2048);
        assert!(p.creates);
    }

    #[test]
    fn refs_lookup() {
        let mut r = Refs::new();
        let n = Name::parse_text("img").unwrap_or(Name::empty());
        assert!(r.add(n, 10).is_ok());
        assert_eq!(r.find(b"img"), Some(10));
        assert_eq!(r.find(b"x"), None);
        assert!(r.add(n, 20).is_ok());
        assert_eq!(r.len(), 1);
        assert_eq!(r.find(b"img"), Some(20));
    }

    #[test]
    fn opts_rules() {
        assert_eq!(parse_opts(&[]), Err(TR_ESIZE));
        assert_eq!(parse_opts(&["-s", "1", "-r", "x"]), Err(TR_ESTATE));
        assert_eq!(parse_opts(&["-s", "1", "-s", "2"]), Err(TR_EDUP));
        assert_eq!(parse_opts(&["--size=1K"]).map(|o| o.size.map(|s| s.bytes)), Ok(Some(1024)));
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.sample(13).map(|s| s.code), Some(TR_ESTATE));
    }
}
