//! UNX-F11599 busybox 分发面 20 applet 抽样对账总表
//!
//! busybox 的分发面只有一条规则：调用名决定 applet，四条路径（以 argv[0]
//! 直调、`--list` 清单、`help` 退出码、`busybox <applet>` 形式）必须落到
//! 同一结论。本模块把这条规则拆成 20 个 applet × 4 个维度 = 80 个抽样点，
//! 每个点记「实测值 got」与「基准值 want」，逐点判一致与否。
//!
//! 维度编码是整型而不是字符串：`Argv0` 记分发成功的 applet 数，`List` 记
//! 清单里出现与否，`Help` 记退出码（0 为正常），`Form` 记 `busybox <applet>`
//! 形式的实际行为码。之所以不用文本，是因为对账表要能在 no_std 下按位
//! 比对、按位求和，整型是唯一不会被 locale 与格式串污染的表征。
//!
//! 一致率按抽样点数计、向上取整：79/80 = 988‰，80/80 = 1000‰。只有
//! 「20 行齐备 + 每行 4 维齐全 + 零分歧」三条同时成立，总表才算绿；
//! 缺行或缺点落 `EMISS`，行数够但卡在 19 行落 `Amber`，出现分歧落 `Red`。
//!
//! 闸门次序：字节 → 名称 → 维度 → 行 → 表 → 对账 → 渲染 → 绿档。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本 14 格
//! （8 成 6 败）在 `audit` 逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfac0;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 抽样 applet 数。
pub const APPS: usize = 20;

/// 每 applet 的维度数。
pub const DIMS: usize = 4;

/// 抽样点数。
pub const PROBES: usize = APPS * DIMS;

/// 名称容量。
pub const NAME_CAP: usize = 16;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 字节非法。
pub const BB_EBYTE: u32 = SEG_BASE + 0x01;
/// 名称为空。
pub const BB_ENAME: u32 = SEG_BASE + 0x02;
/// 编号重复。
pub const BB_EDUP: u32 = SEG_BASE + 0x03;
/// 超长。
pub const BB_ELEN: u32 = SEG_BASE + 0x04;
/// 维度重复。
pub const BB_EDIM: u32 = SEG_BASE + 0x05;
/// 状态冲突。
pub const BB_ESTATE: u32 = SEG_BASE + 0x06;
/// 缺行缺点。
pub const BB_EMISS: u32 = SEG_BASE + 0x07;
/// 未达绿档。
pub const BB_EVER: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const BB_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const BB_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const BB_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const BB_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const BB_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 行装载。
pub const OP_ADD: u8 = 0x01;
/// 抽样点登记。
pub const OP_PROBE: u8 = 0x02;
/// 对账。
pub const OP_JOIN: u8 = 0x03;
/// 一致率。
pub const OP_RATE: u8 = 0x04;
/// 总表渲染。
pub const OP_REPORT: u8 = 0x05;
/// 绿档闸门。
pub const OP_GATE: u8 = 0x06;

// ---------------------------------------------------------------------------
// 哈希
// ---------------------------------------------------------------------------

/// FNV 偏移基。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV 素数。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个字节。
fn fold_byte(h: u64, b: u8) -> u64 {
    let mut v = h ^ (b as u64);
    let mut k = 0usize;
    while k < 8 {
        v = v.wrapping_mul(FNV_PRIME);
        k += 1;
    }
    v
}

/// 折入一个字（大端序逐字节）。
fn fold(h: u64, w: u64) -> u64 {
    let mut v = h;
    let mut shift = 56u32;
    loop {
        v = fold_byte(v, (((w >> shift) & 0xff) as u32) as u8);
        if shift == 0 {
            break;
        }
        shift -= 8;
    }
    v
}

/// 字节串摘要。
fn digest(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFF;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h = fold_byte(h, *b);
        }
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 十进制渲染
// ---------------------------------------------------------------------------

/// 追加一个无符号十进制数。
fn push_dec(out: &mut String, v: u32) {
    if v == 0 {
        out.push('0');
        return;
    }
    let mut n = v;
    let mut buf = [0u8; 10];
    let mut len = 0usize;
    while n > 0 {
        let d = (n % 10) as u8;
        if let Some(dst) = buf.get_mut(len) {
            *dst = b'0' + d;
        }
        n /= 10;
        len += 1;
    }
    while len > 0 {
        len -= 1;
        if let Some(b) = buf.get(len) {
            out.push(*b as char);
        }
    }
}

// ---------------------------------------------------------------------------
// 名称
// ---------------------------------------------------------------------------

/// 定长 applet 名。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Name {
    buf: [u8; NAME_CAP],
    len: usize,
}

impl Name {
    /// 装载：可打印 ASCII，16 字节内。
    pub fn new(bytes: &[u8]) -> Result<Name, u32> {
        if bytes.is_empty() {
            return Err(BB_ENAME);
        }
        if bytes.len() > NAME_CAP {
            return Err(BB_ELEN);
        }
        let mut buf = [0u8; NAME_CAP];
        let mut i = 0usize;
        while i < bytes.len() {
            let b = match bytes.get(i) {
                Some(v) => *v,
                None => return Err(BB_ENOBS),
            };
            if b < 0x20 || b == 0x7f || b >= 0x80 {
                return Err(BB_EBYTE);
            }
            if let Some(dst) = buf.get_mut(i) {
                *dst = b;
            }
            i += 1;
        }
        Ok(Name { buf, len: bytes.len() })
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 字节视图。
    pub fn bytes(&self) -> &[u8] {
        self.buf.get(..self.len).unwrap_or(&[])
    }

    /// 逐字节比对。
    pub fn same(&self, other: &[u8]) -> bool {
        if self.len != other.len() {
            return false;
        }
        let mut i = 0usize;
        while i < self.len {
            match (self.buf.get(i), other.get(i)) {
                (Some(a), Some(b)) => {
                    if a != b {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 文本视图。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(b) = self.buf.get(i) {
                s.push(*b as char);
            }
            i += 1;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 维度
// ---------------------------------------------------------------------------

/// 分发面的四个抽样维度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dim {
    /// argv[0] 直调。
    Argv0,
    /// `--list` 清单成员。
    List,
    /// `help` 退出码。
    Help,
    /// `busybox <applet>` 形式。
    Form,
}

impl Dim {
    /// 短名。
    pub fn text(&self) -> &'static str {
        match self {
            Dim::Argv0 => "argv0",
            Dim::List => "list",
            Dim::Help => "help",
            Dim::Form => "form",
        }
    }

    /// 编码：1..=4。
    pub fn code(&self) -> u8 {
        match self {
            Dim::Argv0 => 1,
            Dim::List => 2,
            Dim::Help => 3,
            Dim::Form => 4,
        }
    }

    /// 列号（0..3）。
    pub fn index(&self) -> usize {
        match self {
            Dim::Argv0 => 0,
            Dim::List => 1,
            Dim::Help => 2,
            Dim::Form => 3,
        }
    }

    /// 按列号反查。
    pub fn at(i: usize) -> Option<Dim> {
        match i {
            0 => Some(Dim::Argv0),
            1 => Some(Dim::List),
            2 => Some(Dim::Help),
            3 => Some(Dim::Form),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// 抽样点
// ---------------------------------------------------------------------------

/// 一个抽样点：实测值与基准值。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Probe {
    dim: Dim,
    got: u32,
    want: u32,
}

impl Probe {
    /// 装载。
    pub fn new(dim: Dim, got: u32, want: u32) -> Probe {
        Probe { dim, got, want }
    }

    /// 维度。
    pub fn dim(&self) -> Dim {
        self.dim
    }

    /// 实测值。
    pub fn got(&self) -> u32 {
        self.got
    }

    /// 基准值。
    pub fn want(&self) -> u32 {
        self.want
    }

    /// 是否一致。
    pub fn agree(&self) -> bool {
        self.got == self.want
    }

    /// 偏差（实测减去基准的绝对值）。
    pub fn delta(&self) -> u32 {
        if self.got >= self.want {
            self.got - self.want
        } else {
            self.want - self.got
        }
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold(h, self.dim.code() as u64);
        h = fold(h, self.got as u64);
        h = fold(h, self.want as u64);
        h
    }
}

// ---------------------------------------------------------------------------
// 行结论
// ---------------------------------------------------------------------------

/// 单行结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowVerdict {
    /// 四维齐全且全一致。
    Match,
    /// 有分歧。
    Diverge,
    /// 维度不全。
    Incomplete,
}

impl RowVerdict {
    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            RowVerdict::Match => "MATCH",
            RowVerdict::Diverge => "DIVERGE",
            RowVerdict::Incomplete => "INCOMPLETE",
        }
    }

    /// 是否不可放行。
    pub fn bad(&self) -> bool {
        match self {
            RowVerdict::Match => false,
            RowVerdict::Diverge => true,
            RowVerdict::Incomplete => true,
        }
    }
}

// ---------------------------------------------------------------------------
// 行
// ---------------------------------------------------------------------------

/// 一张 applet 抽样行：编号 + 名称 + 四维。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    no: u16,
    name: Name,
    probes: [Option<Probe>; DIMS],
    len: usize,
}

impl Row {
    /// 装载：编号不得为 0。
    pub fn new(no: u16, name: &[u8]) -> Result<Row, u32> {
        if no == 0 {
            return Err(BB_EVALUE);
        }
        let n = Name::new(name)?;
        Ok(Row { no, name: n, probes: [None; DIMS], len: 0 })
    }

    /// 编号。
    pub fn no(&self) -> u16 {
        self.no
    }

    /// 名称字节。
    pub fn name(&self) -> &[u8] {
        self.name.bytes()
    }

    /// 名称文本。
    pub fn name_text(&self) -> String {
        self.name.text()
    }

    /// 已登记维数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空行。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取第 i 格。
    pub fn probe(&self, i: usize) -> Option<Probe> {
        if i >= self.len {
            return None;
        }
        self.probes.get(i).copied().unwrap_or(None)
    }

    /// 按维度取。
    pub fn at(&self, dim: Dim) -> Option<Probe> {
        self.probes.get(dim.index()).copied().unwrap_or(None)
    }

    /// 已登记维度里是否含该维。
    pub fn has(&self, dim: Dim) -> bool {
        self.at(dim).is_some()
    }

    /// 挂一个抽样点：同维拒绝，满格拒绝。
    pub fn push(&mut self, p: Probe) -> Result<(), u32> {
        if self.probes.get(p.dim().index()).copied().unwrap_or(None).is_some() {
            return Err(BB_EDIM);
        }
        if self.len >= DIMS {
            return Err(BB_ELEN);
        }
        if let Some(dst) = self.probes.get_mut(self.len) {
            *dst = Some(p);
        }
        self.len += 1;
        Ok(())
    }

    /// 一致点数。
    pub fn agree(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(p)) = self.probes.get(i) {
                if p.agree() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 分歧点数。
    pub fn diverge(&self) -> usize {
        self.len - self.agree()
    }

    /// 偏差总和。
    pub fn delta_sum(&self) -> u32 {
        let mut d = 0u32;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(p)) = self.probes.get(i) {
                d = d.saturating_add(p.delta());
            }
            i += 1;
        }
        d
    }

    /// 四维是否齐全。
    pub fn complete(&self) -> bool {
        self.len == DIMS
    }

    /// 行结论。
    pub fn verdict(&self) -> RowVerdict {
        if !self.complete() {
            RowVerdict::Incomplete
        } else if self.diverge() > 0 {
            RowVerdict::Diverge
        } else {
            RowVerdict::Match
        }
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold(h, self.no as u64);
        let nb = self.name.bytes();
        let mut i = 0usize;
        while i < nb.len() {
            if let Some(b) = nb.get(i) {
                h = fold_byte(h, *b);
            }
            i += 1;
        }
        let mut k = 0usize;
        while k < self.len {
            if let Some(Some(p)) = self.probes.get(k) {
                h = fold(h, p.seal());
            }
            k += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 总表
// ---------------------------------------------------------------------------

/// 20 行抽样总表，按编号升序。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Table {
    rows: [Option<Row>; APPS],
    len: usize,
}

impl Table {
    /// 空表。
    pub fn new() -> Table {
        Table { rows: [None; APPS], len: 0 }
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一行。
    pub fn row(&self, i: usize) -> Option<Row> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied().unwrap_or(None)
    }

    /// 按编号找位。
    pub fn find(&self, no: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(r)) = self.rows.get(i) {
                if r.no() == no {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 是否升序。
    pub fn is_sorted(&self) -> bool {
        let mut i = 1usize;
        while i < self.len {
            let prev = match self.rows.get(i - 1) {
                Some(Some(v)) => *v,
                _ => return false,
            };
            let cur = match self.rows.get(i) {
                Some(Some(v)) => *v,
                _ => return false,
            };
            if prev.no() >= cur.no() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 升序插位入表。
    pub fn add(&mut self, row: Row) -> Result<(), u32> {
        if self.find(row.no()).is_some() {
            return Err(BB_EDUP);
        }
        if self.len >= APPS {
            return Err(BB_ELEN);
        }
        let mut pos = self.len;
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(Some(r)) => {
                    if r.no() > row.no() {
                        pos = i;
                        break;
                    }
                }
                _ => return Err(BB_ESLOT),
            }
            i += 1;
        }
        let mut j = self.len;
        while j > pos {
            let src = match self.rows.get(j - 1).copied() {
                Some(v) => v,
                None => return Err(BB_ESLOT),
            };
            if let Some(dst) = self.rows.get_mut(j) {
                *dst = src;
            }
            j -= 1;
        }
        if let Some(dst) = self.rows.get_mut(pos) {
            *dst = Some(row);
        }
        self.len += 1;
        Ok(())
    }

    /// 已登记抽样点总数。
    pub fn probes(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(r)) = self.rows.get(i) {
                n += r.len();
            }
            i += 1;
        }
        n
    }

    /// 一致点总数。
    pub fn agree(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(r)) = self.rows.get(i) {
                n += r.agree();
            }
            i += 1;
        }
        n
    }

    /// 分歧点总数。
    pub fn diverge(&self) -> usize {
        self.probes() - self.agree()
    }

    /// 覆盖档：20 行齐备且每行四维齐全。
    pub fn complete(&self) -> Result<(), u32> {
        if self.len != APPS {
            return Err(BB_EMISS);
        }
        let mut i = 0usize;
        while i < self.len {
            let r = match self.rows.get(i) {
                Some(Some(v)) => *v,
                _ => return Err(BB_ESLOT),
            };
            if !r.complete() {
                return Err(BB_EMISS);
            }
            i += 1;
        }
        Ok(())
    }

    /// 分歧行编号串（无分歧为 `-`）。
    pub fn red_text(&self) -> String {
        let mut s = String::new();
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(r)) = self.rows.get(i) {
                if r.diverge() > 0 {
                    if n > 0 {
                        s.push(',');
                    }
                    push_dec(&mut s, r.no() as u32);
                    n += 1;
                }
            }
            i += 1;
        }
        if n == 0 {
            s.push('-');
        }
        s
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(r)) = self.rows.get(i) {
                h = fold(h, r.seal());
            }
            i += 1;
        }
        fold(h, self.len as u64)
    }
}

// ---------------------------------------------------------------------------
// 结论
// ---------------------------------------------------------------------------

/// 总表结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 全绿。
    Green,
    /// 覆盖不足。
    Amber,
    /// 有分歧。
    Red,
}

impl Verdict {
    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            Verdict::Green => "GREEN",
            Verdict::Amber => "AMBER",
            Verdict::Red => "RED",
        }
    }

    /// 编码。
    pub fn code(&self) -> u32 {
        match self {
            Verdict::Green => 1,
            Verdict::Amber => 2,
            Verdict::Red => 3,
        }
    }

    /// 是否不可放行。
    pub fn bad(&self) -> bool {
        match self {
            Verdict::Green => false,
            Verdict::Amber => true,
            Verdict::Red => true,
        }
    }
}

// ---------------------------------------------------------------------------
// 对账报告
// ---------------------------------------------------------------------------

/// 对账报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    rows: usize,
    probes: usize,
    agree: usize,
    diverge: usize,
    ok_rows: usize,
    rate: u32,
    dims: [usize; DIMS],
    reds: [u16; APPS],
    red_len: usize,
}

impl Report {
    /// 空报告。
    pub fn new() -> Report {
        Report {
            rows: 0,
            probes: 0,
            agree: 0,
            diverge: 0,
            ok_rows: 0,
            rate: 0,
            dims: [0usize; DIMS],
            reds: [0u16; APPS],
            red_len: 0,
        }
    }

    /// 行数。
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// 抽样点数。
    pub fn probes(&self) -> usize {
        self.probes
    }

    /// 一致点数。
    pub fn agree(&self) -> usize {
        self.agree
    }

    /// 分歧点数。
    pub fn diverge(&self) -> usize {
        self.diverge
    }

    /// 全一致行数。
    pub fn ok_rows(&self) -> usize {
        self.ok_rows
    }

    /// 一致率千分比。
    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// 某维一致点数。
    pub fn dim_agree(&self, dim: Dim) -> usize {
        match self.dims.get(dim.index()) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 红行数。
    pub fn red_len(&self) -> usize {
        self.red_len
    }

    /// 取红行编号。
    pub fn red(&self, i: usize) -> Option<u16> {
        if i >= self.red_len {
            return None;
        }
        self.reds.get(i).copied()
    }

    /// 红行编号串。
    pub fn red_text(&self) -> String {
        let mut s = String::new();
        if self.red_len == 0 {
            s.push('-');
            return s;
        }
        let mut i = 0usize;
        while i < self.red_len {
            if i > 0 {
                s.push(',');
            }
            if let Some(no) = self.reds.get(i) {
                push_dec(&mut s, *no as u32);
            }
            i += 1;
        }
        s
    }

    /// 绿档：零分歧、20 行、80 点。
    pub fn green(&self) -> bool {
        self.diverge == 0 && self.rows == APPS && self.probes == PROBES
    }

    /// 绿档闸门。
    pub fn gate(&self) -> Result<(), u32> {
        if self.green() {
            Ok(())
        } else {
            Err(BB_EVER)
        }
    }

    /// 结论。
    pub fn verdict(&self) -> Verdict {
        if self.diverge > 0 {
            Verdict::Red
        } else if !self.green() {
            Verdict::Amber
        } else {
            Verdict::Green
        }
    }

    /// 单行渲染。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("busybox applets=");
        push_dec(&mut s, self.rows as u32);
        s.push_str(" probes=");
        push_dec(&mut s, self.probes as u32);
        s.push_str(" agree=");
        push_dec(&mut s, self.agree as u32);
        s.push_str(" diverge=");
        push_dec(&mut s, self.diverge as u32);
        s.push_str(" ok=");
        push_dec(&mut s, self.ok_rows as u32);
        s.push_str(" rate=");
        push_dec(&mut s, self.rate);
        s.push_str("ppm reds=");
        let rt = self.red_text();
        s.push_str(rt.as_str());
        s.push('\n');
        s
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold(h, self.rows as u64);
        h = fold(h, self.probes as u64);
        h = fold(h, self.agree as u64);
        h = fold(h, self.diverge as u64);
        h = fold(h, self.ok_rows as u64);
        h = fold(h, self.rate as u64);
        let mut i = 0usize;
        while i < DIMS {
            if let Some(v) = self.dims.get(i) {
                h = fold(h, *v as u64);
            }
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 对账
// ---------------------------------------------------------------------------

/// 把总表折成报告。
pub fn reconcile(t: &Table) -> Result<Report, u32> {
    if t.is_empty() {
        return Err(BB_ESTATE);
    }
    let mut dims = [0usize; DIMS];
    let mut reds = [0u16; APPS];
    let mut red_len = 0usize;
    let mut ok_rows = 0usize;
    let mut i = 0usize;
    while i < t.len() {
        let r = match t.row(i) {
            Some(v) => v,
            None => return Err(BB_ESLOT),
        };
        if r.verdict() == RowVerdict::Match {
            ok_rows += 1;
        } else if red_len < APPS {
            if let Some(dst) = reds.get_mut(red_len) {
                *dst = r.no();
            }
            red_len += 1;
        }
        let mut k = 0usize;
        while k < r.len() {
            let p = match r.probe(k) {
                Some(v) => v,
                None => return Err(BB_ESLOT),
            };
            if p.agree() {
                if let Some(dst) = dims.get_mut(p.dim().index()) {
                    *dst += 1;
                }
            }
            k += 1;
        }
        i += 1;
    }
    let probes = t.probes();
    let agree = t.agree();
    let diverge = probes - agree;
    let rate = if probes == 0 {
        0
    } else {
        ((agree as u64).saturating_mul(1000).saturating_add(probes as u64 - 1) / probes as u64) as u32
    };
    Ok(Report {
        rows: t.len(),
        probes,
        agree,
        diverge,
        ok_rows,
        rate,
        dims,
        reds,
        red_len,
    })
}

/// 一次对账到底的口子。
pub fn reconcile_once() -> Result<Report, u32> {
    let t = sample_table()?;
    reconcile(&t)
}

// ---------------------------------------------------------------------------
// 样本
// ---------------------------------------------------------------------------

/// 20 个 applet 名称。
const APPS20: [&[u8]; APPS] = [
    b"ash",
    b"cat",
    b"cp",
    b"date",
    b"dd",
    b"df",
    b"du",
    b"echo",
    b"find",
    b"grep",
    b"head",
    b"ln",
    b"ls",
    b"mkdir",
    b"mv",
    b"printf",
    b"rm",
    b"sed",
    b"sh",
    b"wc",
];

/// 某维度的基准值。
fn want_of(dim: Dim) -> u32 {
    match dim {
        Dim::Argv0 => 1,
        Dim::List => 1,
        Dim::Help => 0,
        Dim::Form => 1,
    }
}

/// 造一行标准样本（四维全一致）。
pub fn sample_row(no: u16) -> Result<Row, u32> {
    if no == 0 || (no as usize) > APPS {
        return Err(BB_EVALUE);
    }
    let name = match APPS20.get((no as usize) - 1) {
        Some(v) => *v,
        None => return Err(BB_ENOBS),
    };
    let mut r = Row::new(no, name)?;
    let mut i = 0usize;
    while i < DIMS {
        let dim = match Dim::at(i) {
            Some(v) => v,
            None => return Err(BB_EVALUE),
        };
        let w = want_of(dim);
        r.push(Probe::new(dim, w, w))?;
        i += 1;
    }
    Ok(r)
}

/// 20 行全一致样本。
pub fn sample_table() -> Result<Table, u32> {
    let mut t = Table::new();
    let mut no = 1u16;
    while no <= APPS as u16 {
        t.add(sample_row(no)?)?;
        no += 1;
    }
    Ok(t)
}

/// 分歧样本：第 9 行（find）的 form 维实测 2、基准 1。
pub fn sample_diverge() -> Result<Table, u32> {
    let mut t = Table::new();
    let mut no = 1u16;
    while no <= APPS as u16 {
        let mut row = sample_row(no)?;
        if no == 9 {
            let mut fixed = Row::new(row.no(), row.name())?;
            let mut i = 0usize;
            while i < row.len() {
                let p = match row.probe(i) {
                    Some(v) => v,
                    None => return Err(BB_ESLOT),
                };
                let turned = if p.dim() == Dim::Form {
                    Probe::new(Dim::Form, 2, 1)
                } else {
                    p
                };
                fixed.push(turned)?;
                i += 1;
            }
            row = fixed;
        }
        t.add(row)?;
        no += 1;
    }
    Ok(t)
}

/// 缺行样本：只有前 19 行。
pub fn sample_short() -> Result<Table, u32> {
    let mut t = Table::new();
    let mut no = 1u16;
    while no < APPS as u16 {
        t.add(sample_row(no)?)?;
        no += 1;
    }
    Ok(t)
}

// ---------------------------------------------------------------------------
// 账本
// ---------------------------------------------------------------------------

/// 账本一格。
#[derive(Clone, Copy, Debug)]
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
            return Err(BB_ESLOT);
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
            return Err(BB_EBAL);
        }
        const VALS: [u32; 8] = [20, 80, 80, 0, 1000, 1, 19, 1];
        const CODES: [u32; 6] = [BB_EVALUE, BB_EDIM, BB_EDUP, BB_EBYTE, BB_ESTATE, BB_EMISS];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(BB_EBAL),
            };
            if s.slot as usize != i {
                return Err(BB_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(BB_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(BB_EVALUE);
                        }
                    }
                    None => return Err(BB_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(BB_EVALUE);
                        }
                    }
                    None => return Err(BB_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(BB_EBAL);
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
            return Err(BB_EBAL);
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
            Err(BB_ESEAL)
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

/// `Result<u64, u32>` → 样本。
fn long_cell(slot: u8, op: u8, r: Result<u64, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
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

/// 账本组装：8 成 6 败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(count_cell(0, OP_ADD, sample_table().map(|t| t.len())));

    let _ = l.register(count_cell(1, OP_JOIN, sample_table().map(|t| t.probes())));

    let _ = l.register(count_cell(2, OP_JOIN, sample_table().map(|t| t.agree())));

    let _ = l.register(count_cell(3, OP_JOIN, sample_table().map(|t| t.diverge())));

    let _ = l.register(count_cell(
        4,
        OP_RATE,
        sample_table().and_then(|t| reconcile(&t)).map(|r| r.rate as usize),
    ));

    let _ = l.register(count_cell(
        5,
        OP_REPORT,
        sample_diverge().and_then(|t| reconcile(&t)).map(|r| r.red_len),
    ));

    let _ = l.register(count_cell(
        6,
        OP_ADD,
        sample_short().and_then(|t| reconcile(&t)).map(|r| r.rows),
    ));

    let _ = l.register(count_cell(
        7,
        OP_GATE,
        sample_table()
            .and_then(|t| reconcile(&t))
            .map(|r| r.verdict().code() as usize),
    ));

    let _ = l.register(long_cell(
        8,
        OP_ADD,
        Row::new(0, b"ash").map(|_| 0u64),
    ));

    let _ = l.register(long_cell(
        9,
        OP_PROBE,
        sample_row(1)
            .and_then(|mut r| {
                let first = match r.probe(0) {
                    Some(v) => v,
                    None => return Err(BB_ESLOT),
                };
                r.push(first)
            })
            .map(|_| 0u64),
    ));

    let _ = l.register(long_cell(
        10,
        OP_ADD,
        sample_table()
            .and_then(|mut t| {
                let again = match sample_row(1) {
                    Ok(v) => v,
                    Err(e) => return Err(e),
                };
                t.add(again)
            })
            .map(|_| 0u64),
    ));

    let _ = l.register(long_cell(
        11,
        OP_ADD,
        Row::new(1, &[0x80]).map(|_| 0u64),
    ));

    let _ = l.register(long_cell(
        12,
        OP_JOIN,
        reconcile(&Table::new()).map(|_| 0u64),
    ));

    let _ = l.register(long_cell(
        13,
        OP_GATE,
        sample_short()
            .and_then(|t| t.complete())
            .map(|_| 0u64),
    ));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 总表装载面。
fn table_ok() -> bool {
    let t = match sample_table() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = t.len() == APPS && t.is_sorted() && !t.is_empty();
    let b = match (t.row(0), t.row(19)) {
        (Some(x), Some(y)) => {
            x.no() == 1
                && y.no() == 20
                && x.name_text() == String::from("ash")
                && y.name_text() == String::from("wc")
        }
        _ => false,
    };
    let c = t.find(9) == Some(8) && t.find(99).is_none() && t.row(20).is_none();
    let d = t.seal() != Table::new().seal() && t.probes() == PROBES;
    a && b && c && d
}

/// 名称与字节闸门。
fn name_ok() -> bool {
    let a = Name::new(b"").err() == Some(BB_ENAME);
    let b = Name::new(&[0x80]).err() == Some(BB_EBYTE);
    let c = Name::new(&[b'q'; NAME_CAP + 1]).err() == Some(BB_ELEN);
    let d = Name::new(&[0x1f]).err() == Some(BB_EBYTE) && Name::new(&[0x7f]).err() == Some(BB_EBYTE);
    let e = match Name::new(b"printf") {
        Ok(n) => n.len() == 6 && n.same(b"printf") && !n.same(b"print") && n.text() == String::from("printf"),
        Err(_) => false,
    };
    a && b && c && d && e
}

/// 抽样点面。
fn probe_ok() -> bool {
    let a = Probe::new(Dim::Argv0, 1, 1).agree() && Probe::new(Dim::List, 1, 0).agree() == false;
    let b = Probe::new(Dim::Form, 2, 1).delta() == 1 && Probe::new(Dim::Form, 0, 3).delta() == 3;
    let c = Probe::new(Dim::Help, 0, 0).delta() == 0 && Probe::new(Dim::Help, 5, 5).agree();
    let d = Dim::Argv0.text() == "argv0"
        && Dim::List.text() == "list"
        && Dim::Help.text() == "help"
        && Dim::Form.text() == "form";
    let e = Dim::Argv0.code() == 1 && Dim::Form.code() == 4 && Dim::at(2) == Some(Dim::Help) && Dim::at(4).is_none();
    let f = Probe::new(Dim::List, 1, 1).seal() != Probe::new(Dim::List, 0, 1).seal();
    a && b && c && d && e && f
}

/// 行内四维面。
fn row_ok() -> bool {
    let mut r = match sample_row(1) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.len() == DIMS && r.complete() && r.agree() == DIMS && r.diverge() == 0;
    let b = r.verdict() == RowVerdict::Match && !RowVerdict::Match.bad();
    let c = r.has(Dim::Help) && r.at(Dim::Form).map(|p| p.want()) == Some(1);
    let dup = match r.probe(0) {
        Some(v) => v,
        None => return false,
    };
    let d = r.push(dup) == Err(BB_EDIM);
    let e = match Row::new(0, b"ash") {
        Err(BB_EVALUE) => true,
        _ => false,
    } && match Row::new(1, b"") {
        Err(BB_ENAME) => true,
        _ => false,
    };
    let f = r.delta_sum() == 0 && RowVerdict::Diverge.text() == "DIVERGE" && RowVerdict::Incomplete.bad();
    a && b && c && d && e && f
}

/// 表排序面。
fn order_ok() -> bool {
    let mut t = Table::new();
    let a = t.is_empty() && t.len() == 0 && t.row(0).is_none();
    let r3 = match sample_row(3) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let r1 = match sample_row(1) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let r2 = match sample_row(2) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let b = t.add(r3).is_ok() && t.add(r1).is_ok() && t.add(r2).is_ok();
    let c = t.is_sorted() && t.len() == 3;
    let d = match (t.row(0), t.row(1), t.row(2)) {
        (Some(x), Some(y), Some(z)) => x.no() == 1 && y.no() == 2 && z.no() == 3,
        _ => false,
    };
    let e = match sample_row(1) {
        Ok(v) => t.add(v) == Err(BB_EDUP),
        Err(_) => false,
    };
    a && b && c && d && e
}

/// 缺维与覆盖档。
fn missing_ok() -> bool {
    let mut r = match Row::new(1, b"ash") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.push(Probe::new(Dim::Argv0, 1, 1)).is_ok()
        && r.push(Probe::new(Dim::List, 1, 1)).is_ok()
        && r.push(Probe::new(Dim::Help, 0, 0)).is_ok();
    let b = r.len() == 3 && !r.complete() && r.verdict() == RowVerdict::Incomplete;
    let c = r.push(Probe::new(Dim::Form, 1, 1)).is_ok() && r.complete() && r.verdict() == RowVerdict::Match;
    let full = match sample_table() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let d = full.complete() == Ok(());
    let short = match sample_short() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let e = short.len() == APPS - 1 && short.complete() == Err(BB_EMISS);
    let mut partial = Table::new();
    partial.add(r).expect("add");
    let f = partial.complete() == Err(BB_EMISS);
    a && b && c && d && e && f
}

/// 一致计数面。
fn agree_ok() -> bool {
    let full = match sample_table() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = full.agree() == PROBES && full.diverge() == 0 && full.probes() == PROBES;
    let div = match sample_diverge() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let b = div.agree() == PROBES - 1 && div.diverge() == 1;
    let c = match div.row(8) {
        Some(r) => r.agree() == DIMS - 1 && r.diverge() == 1 && r.delta_sum() == 1,
        None => false,
    };
    let r = match reconcile(&div) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let d = r.dim_agree(Dim::Argv0) == APPS
        && r.dim_agree(Dim::List) == APPS
        && r.dim_agree(Dim::Help) == APPS
        && r.dim_agree(Dim::Form) == APPS - 1;
    a && b && c && d
}

/// 一致率面。
fn rate_ok() -> bool {
    let a = match reconcile_once() {
        Ok(r) => r.rate() == 1000,
        Err(_) => false,
    };
    let b = match sample_diverge().and_then(|t| reconcile(&t)) {
        Ok(r) => r.rate() == 988,
        Err(_) => false,
    };
    let c = match sample_short().and_then(|t| reconcile(&t)) {
        Ok(r) => r.rate() == 1000 && r.probes() == (APPS - 1) * DIMS,
        Err(_) => false,
    };
    let d = Report::new().rate() == 0;
    a && b && c && d
}

/// 对账报告面。
fn report_ok() -> bool {
    let r = match reconcile_once() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.rows() == APPS && r.probes() == PROBES && r.agree() == PROBES && r.diverge() == 0;
    let b = r.ok_rows() == APPS && r.red_len() == 0 && r.red_text() == String::from("-");
    let c = r.render() == String::from("busybox applets=20 probes=80 agree=80 diverge=0 ok=20 rate=1000ppm reds=-\n");
    let d = r.green() && r.gate() == Ok(()) && r.verdict() == Verdict::Green && r.verdict().code() == 1;
    let e = r.seal() != Report::new().seal();
    a && b && c && d && e
}

/// 分歧显式面。
fn reds_ok() -> bool {
    let r = match sample_diverge().and_then(|t| reconcile(&t)) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.rows() == APPS && r.probes() == PROBES && r.agree() == PROBES - 1 && r.diverge() == 1;
    let b = r.ok_rows() == APPS - 1 && r.rate() == 988;
    let c = r.red_len() == 1 && r.red(0) == Some(9) && r.red(1).is_none() && r.red_text() == String::from("9");
    let d = !r.green() && r.gate() == Err(BB_EVER) && r.verdict() == Verdict::Red;
    let e = r.render()
        == String::from("busybox applets=20 probes=80 agree=79 diverge=1 ok=19 rate=988ppm reds=9\n");
    a && b && c && d && e
}

/// 覆盖档面。
fn cover_ok() -> bool {
    let short = match sample_short() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = short.complete() == Err(BB_EMISS) && short.len() == APPS - 1;
    let r = match reconcile(&short) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let b = r.rows() == APPS - 1 && r.probes() == (APPS - 1) * DIMS && r.agree() == (APPS - 1) * DIMS;
    let c = r.diverge() == 0 && r.rate() == 1000 && r.ok_rows() == APPS - 1;
    let d = r.verdict() == Verdict::Amber && r.verdict().code() == 2 && r.gate() == Err(BB_EVER) && !r.green();
    let e = r.render()
        == String::from("busybox applets=19 probes=76 agree=76 diverge=0 ok=19 rate=1000ppm reds=-\n");
    let f = !Verdict::Green.bad() && Verdict::Amber.bad() && Verdict::Red.bad();
    a && b && c && d && e && f
}

/// 容量与空表面。
fn bounds_ok() -> bool {
    let e = Report::new();
    let a = e.rows() == 0 && e.probes() == 0 && e.rate() == 0 && !e.green() && e.red(0).is_none();
    let b = e.render() == String::from("busybox applets=0 probes=0 agree=0 diverge=0 ok=0 rate=0ppm reds=-\n");
    let c = reconcile(&Table::new()) == Err(BB_ESTATE);
    let d = match sample_row(0) {
        Err(BB_EVALUE) => true,
        _ => false,
    } && match sample_row(21) {
        Err(BB_EVALUE) => true,
        _ => false,
    };
    let f = match sample_row(20) {
        Ok(r) => r.name_text() == String::from("wc") && r.complete(),
        Err(_) => false,
    };
    let g = Table::new().complete() == Err(BB_EMISS) && Table::new().red_text() == String::from("-");
    a && b && c && d && f && g
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = table_ok() && name_ok();
    out[1] = probe_ok() && row_ok() && order_ok();
    out[2] = missing_ok() && agree_ok() && rate_ok();
    out[3] = report_ok() && reds_ok() && cover_ok() && bounds_ok() && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11599_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11599.busybox_samples");

    s.add(
        "BB01-总表装载",
        {
            table_ok()
        },
        "static",
    );

    s.add(
        "BB02-名称与字节",
        {
            name_ok()
        },
        "static",
    );

    s.add(
        "BB03-抽样点",
        {
            probe_ok()
        },
        "static",
    );

    s.add(
        "BB04-行内四维",
        {
            row_ok()
        },
        "static",
    );

    s.add(
        "BB05-表内排序",
        {
            order_ok()
        },
        "static",
    );

    s.add(
        "BB06-缺维与覆盖",
        {
            missing_ok()
        },
        "static",
    );

    s.add(
        "BB07-一致计数",
        {
            agree_ok()
        },
        "static",
    );

    s.add(
        "BB08-一致率",
        {
            rate_ok()
        },
        "static",
    );

    s.add(
        "BB09-对账报告",
        {
            report_ok()
        },
        "static",
    );

    s.add(
        "BB10-分歧显式",
        {
            reds_ok()
        },
        "static",
    );

    s.add(
        "BB11-覆盖档",
        {
            cover_ok()
        },
        "static",
    );

    s.add(
        "BB12-容量与空表",
        {
            bounds_ok()
        },
        "static",
    );

    s.add(
        "BB13-冻结重放四注入",
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
    fn table_shape() {
        let t = sample_table().expect("t");
        assert_eq!(t.len(), APPS);
        assert_eq!(t.probes(), PROBES);
        assert!(t.is_sorted());
        assert_eq!(t.find(1), Some(0));
        assert_eq!(t.find(20), Some(19));
        assert_eq!(t.find(21), None);
        assert_eq!(t.row(0).map(|r| r.name_text()), Some(String::from("ash")));
        assert_eq!(t.row(19).map(|r| r.name_text()), Some(String::from("wc")));
    }

    #[test]
    fn row_gates() {
        assert_eq!(Row::new(0, b"ash").err(), Some(BB_EVALUE));
        assert_eq!(Row::new(1, b"").err(), Some(BB_ENAME));
        assert_eq!(Row::new(1, &[0x80]).err(), Some(BB_EBYTE));
        let r = sample_row(1).expect("r");
        assert_eq!(r.len(), DIMS);
        assert_eq!(r.agree(), DIMS);
        assert_eq!(r.diverge(), 0);
        assert!(r.complete());
        assert_eq!(r.verdict(), RowVerdict::Match);
    }

    #[test]
    fn probe_gates() {
        assert!(Probe::new(Dim::Argv0, 1, 1).agree());
        assert!(!Probe::new(Dim::List, 1, 0).agree());
        assert_eq!(Probe::new(Dim::Form, 2, 1).delta(), 1);
        assert_eq!(Probe::new(Dim::Form, 0, 3).delta(), 3);
        assert_eq!(Dim::at(1), Some(Dim::List));
        assert_eq!(Dim::at(9), None);
        assert_eq!(Dim::Help.code(), 3);
    }

    #[test]
    fn duplicate_dim() {
        let mut r = sample_row(1).expect("r");
        let first = r.probe(0).expect("p");
        assert_eq!(r.push(first), Err(BB_EDIM));
        let mut hollow = Row::new(2, b"cat").expect("h");
        assert_eq!(hollow.len(), 0);
        hollow.push(Probe::new(Dim::Argv0, 1, 1)).expect("a");
        hollow.push(Probe::new(Dim::List, 1, 1)).expect("b");
        hollow.push(Probe::new(Dim::Help, 0, 0)).expect("c");
        assert!(!hollow.complete());
        assert_eq!(hollow.verdict(), RowVerdict::Incomplete);
        hollow.push(Probe::new(Dim::Form, 1, 1)).expect("d");
        assert!(hollow.complete());
        assert_eq!(hollow.push(Probe::new(Dim::Form, 1, 1)), Err(BB_EDIM));
    }

    #[test]
    fn table_insert() {
        let mut t = Table::new();
        t.add(sample_row(3).expect("3")).expect("a3");
        t.add(sample_row(1).expect("1")).expect("a1");
        assert!(t.is_sorted());
        assert_eq!(t.row(0).map(|r| r.no()), Some(1));
        assert_eq!(t.row(1).map(|r| r.no()), Some(3));
        assert_eq!(t.add(sample_row(1).expect("1b")), Err(BB_EDUP));
    }

    #[test]
    fn table_capacity() {
        let t = sample_table().expect("t");
        let mut full = t;
        let extra = Row::new(21, b"x21").expect("21");
        assert_eq!(full.add(extra), Err(BB_ELEN));
        assert_eq!(full.len(), APPS);
    }

    #[test]
    fn reconcile_full() {
        let r = reconcile_once().expect("r");
        assert_eq!(r.rows(), APPS);
        assert_eq!(r.probes(), PROBES);
        assert_eq!(r.agree(), PROBES);
        assert_eq!(r.diverge(), 0);
        assert_eq!(r.ok_rows(), APPS);
        assert_eq!(r.rate(), 1000);
        assert_eq!(r.verdict(), Verdict::Green);
        assert_eq!(r.gate(), Ok(()));
        assert_eq!(
            r.render().as_str(),
            "busybox applets=20 probes=80 agree=80 diverge=0 ok=20 rate=1000ppm reds=-\n"
        );
    }

    #[test]
    fn reconcile_diverge() {
        let t = sample_diverge().expect("t");
        let r = reconcile(&t).expect("r");
        assert_eq!(r.agree(), PROBES - 1);
        assert_eq!(r.diverge(), 1);
        assert_eq!(r.ok_rows(), APPS - 1);
        assert_eq!(r.rate(), 988);
        assert_eq!(r.red_len(), 1);
        assert_eq!(r.red(0), Some(9));
        assert_eq!(r.red_text(), String::from("9"));
        assert_eq!(r.verdict(), Verdict::Red);
        assert_eq!(r.gate(), Err(BB_EVER));
    }

    #[test]
    fn reconcile_short() {
        let t = sample_short().expect("t");
        assert_eq!(t.len(), APPS - 1);
        assert_eq!(t.complete(), Err(BB_EMISS));
        let r = reconcile(&t).expect("r");
        assert_eq!(r.rows(), APPS - 1);
        assert_eq!(r.probes(), (APPS - 1) * DIMS);
        assert_eq!(r.rate(), 1000);
        assert_eq!(r.verdict(), Verdict::Amber);
        assert_eq!(r.gate(), Err(BB_EVER));
    }

    #[test]
    fn dim_agreement() {
        let t = sample_diverge().expect("t");
        let r = reconcile(&t).expect("r");
        assert_eq!(r.dim_agree(Dim::Argv0), APPS);
        assert_eq!(r.dim_agree(Dim::List), APPS);
        assert_eq!(r.dim_agree(Dim::Help), APPS);
        assert_eq!(r.dim_agree(Dim::Form), APPS - 1);
    }

    #[test]
    fn empty_report() {
        let e = Report::new();
        assert_eq!(e.rows(), 0);
        assert_eq!(e.rate(), 0);
        assert_eq!(
            e.render().as_str(),
            "busybox applets=0 probes=0 agree=0 diverge=0 ok=0 rate=0ppm reds=-\n"
        );
        assert_eq!(e.red_text(), String::from("-"));
        assert_eq!(reconcile(&Table::new()), Err(BB_ESTATE));
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.sample(0).map(|s| s.value), Some(20));
        assert_eq!(l.sample(4).map(|s| s.value), Some(1000));
        assert_eq!(l.sample(9).map(|s| s.code), Some(BB_EDIM));
        assert_eq!(l.sample(13).map(|s| s.code), Some(BB_EMISS));
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = l.freeze().expect("freeze");
        assert!(l.is_frozen());
        assert_eq!(l.root(), root);
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze().err(), Some(BB_EBAL));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn injections() {
        let four = inject_four();
        assert!(four[0] && four[1] && four[2] && four[3]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11599_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }
}
