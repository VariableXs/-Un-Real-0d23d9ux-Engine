//! UNX-F11545 df 文件系统空间（-h/-i/-T）
//!
//! `df` 的挂载表模型：一张挂载表按「设备 → 挂载点 → 文件系统型 → 容量 →
//! inode」五段逐段装载，装载后再派生「可用块」「已用百分比」「可用 inode」
//! 三个派生面。渲染面支持三档表体：
//!
//! * 缺省档：`Filesystem 1K-blocks Used Available Use% Mounted on` 六列；
//! * inode 档（`-i`）：把容量四列换成 `Inodes IUsed IFree IUse%`；
//! * 型列档（`-T`）：在文件系统列后插入 `Type`，六列变七列。
//!
//! 单位面三档：`-k` 千块（缺省）、`-m` 兆块（向上取整）、`-h` 人读（最大
//! 单位下不足十者留一位小数，其余向上取整）。可见性面：伪文件系统
//! （tmpfs/proc）缺省隐藏，`-a` 全出。操作数面：路径按「最长挂载点前缀」
//! 归属，落表时同一挂载点只出现一次。
//!
//! 闸门次序：字节 → 名称 → 挂载 → 容量 → 类型 → 选项 → 列宽 → 百分比 → 表格。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本 14 格
//! （8 成 6 败）在 `audit` 逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf320;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 挂载表容量。
pub const MT_CAP: usize = 8;

/// 表列容量。
pub const COLS_CAP: usize = 7;

/// 表体行容量（不含表头）。
pub const TABLE_CAP: usize = 8;

/// 名称容量。
pub const NAME_CAP: usize = 24;

/// 操作数容量。
pub const PATH_CAP: usize = 4;

/// 单元行容量。
pub const LINE_CAP: usize = 160;

/// 一千。
pub const KIB: u64 = 1024;

/// 一百万。
pub const MIB: u64 = 1024 * 1024;

/// 十亿。
pub const GIB: u64 = 1024 * 1024 * 1024;

/// 一万亿。
pub const TIB: u64 = 1024 * 1024 * 1024 * 1024;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 字节非法。
pub const DF_EBYTE: u32 = SEG_BASE + 0x01;
/// 名称为空。
pub const DF_ENAME: u32 = SEG_BASE + 0x02;
/// 挂载点非法。
pub const DF_EMOUNT: u32 = SEG_BASE + 0x03;
/// 容量矛盾。
pub const DF_ECAP: u32 = SEG_BASE + 0x04;
/// 类型非法。
pub const DF_ETYPE: u32 = SEG_BASE + 0x05;
/// 选项非法。
pub const DF_EOPT: u32 = SEG_BASE + 0x06;
/// 超长。
pub const DF_ELEN: u32 = SEG_BASE + 0x07;
/// 状态冲突。
pub const DF_ESTATE: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const DF_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const DF_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const DF_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const DF_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const DF_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 挂载装载。
pub const OP_MOUNT: u8 = 0x01;
/// 容量派生。
pub const OP_CAP: u8 = 0x02;
/// 百分比。
pub const OP_PCT: u8 = 0x03;
/// 人读换算。
pub const OP_HUMAN: u8 = 0x04;
/// 表格渲染。
pub const OP_TABLE: u8 = 0x05;
/// 选项解析。
pub const OP_OPT: u8 = 0x06;

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

/// 是否可打印：0x21..=0x7e。
fn printable(s: &[u8]) -> bool {
    let mut i = 0usize;
    while i < s.len() {
        match s.get(i) {
            Some(b) => {
                if *b < 0x21 || *b > 0x7e {
                    return false;
                }
            }
            None => return false,
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

/// 十进制追加。
fn push_u64(s: &mut String, mut v: u64) {
    let mut buf = [0u8; 20];
    let mut at = buf.len();
    if v == 0 {
        s.push('0');
        return;
    }
    while v > 0 && at > 0 {
        at -= 1;
        let d = (v % 10) as u8;
        if let Some(dst) = buf.get_mut(at) {
            *dst = b'0' + d;
        }
        v /= 10;
    }
    let mut i = at;
    while i < buf.len() {
        if let Some(b) = buf.get(i) {
            s.push(*b as char);
        }
        i += 1;
    }
}

/// 向上取整除法。
fn div_ceil(a: u64, b: u64) -> u64 {
    if b == 0 {
        return a;
    }
    a / b + if a % b != 0 { 1 } else { 0 }
}

/// 百分比进一，封顶一百。
fn percent(part: u64, whole: u64) -> u32 {
    if whole == 0 {
        return 0;
    }
    let num = part.saturating_mul(100);
    let mut p = num / whole + if num % whole != 0 { 1 } else { 0 };
    if p > 100 {
        p = 100;
    }
    p as u32
}

// ---------------------------------------------------------------------------
// 名称
// ---------------------------------------------------------------------------

/// 定长名称。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Name {
    buf: [u8; NAME_CAP],
    len: u8,
}

impl Name {
    /// 空白名称。
    pub const fn blank() -> Name {
        Name { buf: [0u8; NAME_CAP], len: 0 }
    }

    /// 装载：非空、可打印、不超长。
    pub fn new(s: &[u8]) -> Result<Name, u32> {
        if s.is_empty() {
            return Err(DF_ENAME);
        }
        if s.len() > NAME_CAP {
            return Err(DF_ELEN);
        }
        if !printable(s) {
            return Err(DF_EBYTE);
        }
        let mut n = Name::blank();
        let mut i = 0usize;
        while i < s.len() {
            match (s.get(i), n.buf.get_mut(i)) {
                (Some(src), Some(dst)) => *dst = *src,
                _ => return Err(DF_ELEN),
            }
            i += 1;
        }
        n.len = s.len() as u8;
        Ok(n)
    }

    /// 字节面。
    pub fn bytes(&self) -> &[u8] {
        self.buf.get(0..self.len as usize).unwrap_or(&[])
    }

    /// 文本面。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(b) = self.buf.get(i) {
                s.push(*b as char);
            }
            i += 1;
        }
        s
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 同名判定。
    pub fn named(&self, other: &[u8]) -> bool {
        eq_bytes(self.bytes(), other)
    }

    /// 首字节。
    pub fn first(&self) -> Option<u8> {
        self.buf.get(0).copied()
    }
}

// ---------------------------------------------------------------------------
// 文件系统型
// ---------------------------------------------------------------------------

/// 文件系统型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fs {
    /// ext4。
    Ext4,
    /// xfs。
    Xfs,
    /// tmpfs。
    Tmpfs,
    /// btrfs。
    Btrfs,
    /// proc。
    Proc,
    /// 其它。
    Other,
}

impl Fs {
    /// 文本面。
    pub fn text(&self) -> &'static str {
        match self {
            Fs::Ext4 => "ext4",
            Fs::Xfs => "xfs",
            Fs::Tmpfs => "tmpfs",
            Fs::Btrfs => "btrfs",
            Fs::Proc => "proc",
            Fs::Other => "unknown",
        }
    }

    /// 是否伪文件系统：无实体块设备者。
    pub fn pseudo(&self) -> bool {
        match self {
            Fs::Tmpfs => true,
            Fs::Proc => true,
            _ => false,
        }
    }

    /// 是否有序日志型。
    pub fn journaled(&self) -> bool {
        match self {
            Fs::Ext4 => true,
            Fs::Xfs => true,
            Fs::Btrfs => true,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 挂载项
// ---------------------------------------------------------------------------

/// 一条挂载记录，容量一律以千字节块计。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mount {
    dev: Name,
    dir: Name,
    fs: Fs,
    total: u64,
    used: u64,
    inode_total: u64,
    inode_used: u64,
    bs: u32,
}

impl Mount {
    /// 装载：名称 → 挂载点 → 类型 → 容量，四段闸门。
    pub fn new(
        dev: &[u8],
        dir: &[u8],
        fs: Fs,
        total: u64,
        used: u64,
        inode_total: u64,
        inode_used: u64,
        bs: u32,
    ) -> Result<Mount, u32> {
        let d = Name::new(dev)?;
        let p = Name::new(dir)?;
        if p.first() != Some(b'/') {
            return Err(DF_EMOUNT);
        }
        if bs == 0 {
            return Err(DF_ETYPE);
        }
        if total == 0 || inode_total == 0 {
            return Err(DF_ECAP);
        }
        if used > total || inode_used > inode_total {
            return Err(DF_ECAP);
        }
        Ok(Mount { dev: d, dir: p, fs, total, used, inode_total, inode_used, bs })
    }

    /// 设备名。
    pub fn dev(&self) -> &[u8] {
        self.dev.bytes()
    }

    /// 挂载点。
    pub fn dir(&self) -> &[u8] {
        self.dir.bytes()
    }

    /// 设备名文本。
    pub fn dev_text(&self) -> String {
        self.dev.text()
    }

    /// 挂载点文本。
    pub fn dir_text(&self) -> String {
        self.dir.text()
    }

    /// 文件系统型。
    pub fn fs(&self) -> Fs {
        self.fs
    }

    /// 总块。
    pub fn total(&self) -> u64 {
        self.total
    }

    /// 已用块。
    pub fn used(&self) -> u64 {
        self.used
    }

    /// 可用块：总减已用，饱和。
    pub fn free(&self) -> u64 {
        self.total.saturating_sub(self.used)
    }

    /// 已用百分比，进一。
    pub fn pct(&self) -> u32 {
        percent(self.used, self.total)
    }

    /// 字节容量。
    pub fn bytes(&self) -> u64 {
        self.total.saturating_mul(self.bs as u64)
    }

    /// 总 inode。
    pub fn inode_total(&self) -> u64 {
        self.inode_total
    }

    /// 已用 inode。
    pub fn inode_used(&self) -> u64 {
        self.inode_used
    }

    /// 可用 inode。
    pub fn inode_free(&self) -> u64 {
        self.inode_total.saturating_sub(self.inode_used)
    }

    /// 已用 inode 百分比，进一。
    pub fn inode_pct(&self) -> u32 {
        percent(self.inode_used, self.inode_total)
    }

    /// 块尺寸。
    pub fn block_size(&self) -> u32 {
        self.bs
    }

    /// 同挂载点判定。
    pub fn same_dir(&self, other: &[u8]) -> bool {
        self.dir.named(other)
    }
}

// ---------------------------------------------------------------------------
// 挂载表
// ---------------------------------------------------------------------------

/// 挂载表。
#[derive(Clone, Copy, Debug)]
pub struct Mounts {
    rows: [Option<Mount>; MT_CAP],
    len: usize,
}

impl Mounts {
    /// 空表。
    pub const fn new() -> Mounts {
        Mounts { rows: [None; MT_CAP], len: 0 }
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
    pub fn at(&self, i: usize) -> Option<Mount> {
        if i >= self.len {
            return None;
        }
        match self.rows.get(i) {
            Some(v) => *v,
            None => None,
        }
    }

    /// 追加：满则超长，同挂载点则挂载冲突。
    pub fn push(&mut self, m: Mount) -> Result<u16, u32> {
        if self.len >= MT_CAP {
            return Err(DF_ELEN);
        }
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(old)) = self.rows.get(i) {
                if old.same_dir(m.dir()) {
                    return Err(DF_EMOUNT);
                }
            }
            i += 1;
        }
        match self.rows.get_mut(self.len) {
            Some(dst) => *dst = Some(m),
            None => return Err(DF_ELEN),
        }
        let idx = self.len as u16;
        self.len += 1;
        Ok(idx)
    }

    /// 总块合计。
    pub fn total_blocks(&self) -> u64 {
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(m)) = self.rows.get(i) {
                sum = sum.saturating_add(m.total());
            }
            i += 1;
        }
        sum
    }

    /// 已用块合计。
    pub fn total_used(&self) -> u64 {
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(m)) = self.rows.get(i) {
                sum = sum.saturating_add(m.used());
            }
            i += 1;
        }
        sum
    }

    /// 可用块合计。
    pub fn total_free(&self) -> u64 {
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(m)) = self.rows.get(i) {
                sum = sum.saturating_add(m.free());
            }
            i += 1;
        }
        sum
    }

    /// 可见条数。
    pub fn visible(&self, opts: &Options) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(m)) = self.rows.get(i) {
                if opts.all || !m.fs().pseudo() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 最长挂载点归属。
    pub fn resolve(&self, operand: &[u8]) -> Option<usize> {
        let mut best: Option<usize> = None;
        let mut best_len = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(m)) = self.rows.get(i) {
                if under(m.dir(), operand) {
                    let l = m.dir().len();
                    if best.is_none() || l > best_len {
                        best = Some(i);
                        best_len = l;
                    }
                }
            }
            i += 1;
        }
        best
    }

    /// 全表最大块尺寸。
    pub fn max_block_size(&self) -> u32 {
        let mut mx = 0u32;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(m)) = self.rows.get(i) {
                if m.block_size() > mx {
                    mx = m.block_size();
                }
            }
            i += 1;
        }
        mx
    }

    /// 是否同一设备重复。
    pub fn dev_count(&self, dev: &[u8]) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(m)) = self.rows.get(i) {
                if eq_bytes(m.dev(), dev) {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

/// `operand` 是否落在挂载点 `dir` 之下。
fn under(dir: &[u8], operand: &[u8]) -> bool {
    if eq_bytes(dir, operand) {
        return true;
    }
    if dir.len() > operand.len() {
        return false;
    }
    let mut i = 0usize;
    while i < dir.len() {
        match (dir.get(i), operand.get(i)) {
            (Some(a), Some(b)) => {
                if a != b {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    if dir.len() == 1 && dir.first() == Some(&b'/') {
        return true;
    }
    matches!(operand.get(dir.len()), Some(b'/'))
}

// ---------------------------------------------------------------------------
// 人读单位
// ---------------------------------------------------------------------------

/// 千字节块数 → 人读文本。
///
/// 取最大单位使数值不小于一；数值不足十者留一位小数（进一），其余取整
/// （进一）。因此 1_000_000 块为 `977M`，8_000_000 块为 `7.7G`。
pub fn human(blocks: u64) -> String {
    let units = [b'K', b'M', b'G', b'T'];
    if blocks == 0 {
        return String::from("0K");
    }
    let mut n = 0usize;
    while n < 3 && (blocks >> (10 * (n + 1))) >= 1 {
        n += 1;
    }
    let den = 1u64 << (10 * n);
    let num = blocks.saturating_mul(10);
    let tenths = num / den + if num % den != 0 { 1 } else { 0 };
    let mut out = String::new();
    if tenths < 100 {
        push_u64(&mut out, tenths / 10);
        out.push('.');
        push_u64(&mut out, tenths % 10);
    } else {
        push_u64(&mut out, div_ceil(blocks, den));
    }
    if let Some(u) = units.get(n) {
        out.push(*u as char);
    }
    out
}

// ---------------------------------------------------------------------------
// 选项
// ---------------------------------------------------------------------------

/// 容量单位档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    /// 千块。
    Kilo,
    /// 兆块。
    Mega,
    /// 人读。
    Human,
}

impl Unit {
    /// 表头文本。
    pub fn head(&self) -> &'static str {
        match self {
            Unit::Kilo => "1K-blocks",
            Unit::Mega => "1M-blocks",
            Unit::Human => "Size",
        }
    }

    /// 块数 → 单元格文本。
    pub fn cell(&self, blocks: u64) -> String {
        match self {
            Unit::Human => human(blocks),
            Unit::Kilo => {
                let mut s = String::new();
                push_u64(&mut s, blocks);
                s
            }
            Unit::Mega => {
                let mut s = String::new();
                push_u64(&mut s, div_ceil(blocks, KIB));
                s
            }
        }
    }
}

/// 选项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// 容量单位。
    pub unit: Unit,
    /// inode 档。
    pub inode: bool,
    /// 型列档。
    pub fstype: bool,
    /// 全出档。
    pub all: bool,
}

impl Options {
    /// 缺省：千块、六列、隐藏伪文件系统。
    pub const fn new() -> Options {
        Options { unit: Unit::Kilo, inode: false, fstype: false, all: false }
    }

    /// 列数。
    pub fn cols(&self) -> usize {
        if self.fstype {
            7
        } else {
            6
        }
    }

    /// 文本面，便于测试观察。
    pub fn text(&self) -> String {
        let mut s = String::new();
        s.push_str(self.unit.head());
        if self.inode {
            s.push_str(" +i");
        }
        if self.fstype {
            s.push_str(" +T");
        }
        if self.all {
            s.push_str(" +a");
        }
        s
    }
}

/// 设定单位，同类重复允许，异类冲突。
fn set_unit(cur: &mut Unit, seen: &mut u8, u: Unit) -> Result<(), u32> {
    if *seen > 0 && *cur != u {
        return Err(DF_ESTATE);
    }
    *cur = u;
    *seen = seen.saturating_add(1);
    Ok(())
}

/// 解析命令行。
pub fn parse_argv(args: &[&[u8]]) -> Result<(Options, [Name; PATH_CAP], usize), u32> {
    let mut opts = Options::new();
    let mut names = [Name::blank(); PATH_CAP];
    let mut nlen = 0usize;
    let mut seen = 0u8;
    let mut i = 0usize;
    while i < args.len() {
        let a = match args.get(i) {
            Some(v) => *v,
            None => return Err(DF_EOPT),
        };
        i += 1;
        if a.len() >= 2 && a.first() == Some(&b'-') {
            if matches!(a.get(1), Some(b'-')) {
                return Err(DF_EOPT);
            }
            let mut k = 1usize;
            while k < a.len() {
                match a.get(k) {
                    Some(b'h') => set_unit(&mut opts.unit, &mut seen, Unit::Human)?,
                    Some(b'k') => set_unit(&mut opts.unit, &mut seen, Unit::Kilo)?,
                    Some(b'm') => set_unit(&mut opts.unit, &mut seen, Unit::Mega)?,
                    Some(b'i') => opts.inode = true,
                    Some(b'T') => opts.fstype = true,
                    Some(b'a') => opts.all = true,
                    Some(_) => return Err(DF_EOPT),
                    None => return Err(DF_EOPT),
                }
                k += 1;
            }
            continue;
        }
        if eq_bytes(a, b"-") {
            return Err(DF_EBYTE);
        }
        if nlen >= PATH_CAP {
            return Err(DF_ELEN);
        }
        let nm = Name::new(a)?;
        if let Some(dst) = names.get_mut(nlen) {
            *dst = nm;
        }
        nlen += 1;
    }
    Ok((opts, names, nlen))
}

// ---------------------------------------------------------------------------
// 表格
// ---------------------------------------------------------------------------

/// 定宽表格。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    head: [String; COLS_CAP],
    body: [[String; COLS_CAP]; TABLE_CAP],
    rows: usize,
    cols: usize,
    widths: [usize; COLS_CAP],
}

impl Table {
    /// 空表。
    pub fn new() -> Table {
        Table {
            head: core::array::from_fn(|_| String::new()),
            body: core::array::from_fn(|_| core::array::from_fn(|_| String::new())),
            rows: 0,
            cols: 0,
            widths: [0usize; COLS_CAP],
        }
    }

    /// 列数。
    pub fn cols(&self) -> usize {
        self.cols
    }

    /// 表体行数。
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.rows == 0
    }

    /// 第 c 列表头（未补白）。
    pub fn head(&self, c: usize) -> Option<&str> {
        if c >= self.cols {
            return None;
        }
        self.head.get(c).map(|s| s.as_str())
    }

    /// 第 r 行第 c 列单元格（未补白）。
    pub fn cell(&self, r: usize, c: usize) -> Option<&str> {
        if r >= self.rows || c >= self.cols {
            return None;
        }
        match self.body.get(r).and_then(|row| row.get(c)) {
            Some(s) => Some(s.as_str()),
            None => None,
        }
    }

    /// 第 c 列宽度。
    pub fn width(&self, c: usize) -> Option<usize> {
        if c >= self.cols {
            return None;
        }
        self.widths.get(c).copied()
    }

    /// 设表头。
    fn set_head(&mut self, c: usize, s: &str) -> Result<(), u32> {
        if c >= COLS_CAP || s.len() > LINE_CAP {
            return Err(DF_ELEN);
        }
        match self.head.get_mut(c) {
            Some(dst) => {
                dst.push_str(s);
                Ok(())
            }
            None => Err(DF_ELEN),
        }
    }

    /// 设单元格。
    fn set_cell(&mut self, r: usize, c: usize, s: &str) -> Result<(), u32> {
        if c >= COLS_CAP || s.len() > LINE_CAP {
            return Err(DF_ELEN);
        }
        let row = match self.body.get_mut(r) {
            Some(v) => v,
            None => return Err(DF_ELEN),
        };
        match row.get_mut(c) {
            Some(dst) => {
                dst.push_str(s);
                Ok(())
            }
            None => Err(DF_ELEN),
        }
    }

    /// 量宽：每列取表头与各行的最大字节长。
    fn measure(&mut self) {
        let mut c = 0usize;
        while c < self.cols {
            let mut w = 0usize;
            if let Some(h) = self.head.get(c) {
                w = h.as_bytes().len();
            }
            let mut r = 0usize;
            while r < self.rows {
                if let Some(s) = self.body.get(r).and_then(|row| row.get(c)) {
                    let l = s.as_bytes().len();
                    if l > w {
                        w = l;
                    }
                }
                r += 1;
            }
            if let Some(dst) = self.widths.get_mut(c) {
                *dst = w;
            }
            c += 1;
        }
    }

    /// 单元格文本抽取。
    fn text_at(&self, r: usize, c: usize) -> Option<&str> {
        if r == 0 {
            if c >= self.cols {
                return None;
            }
            return self.head.get(c).map(|s| s.as_str());
        }
        self.cell(r - 1, c)
    }

    /// 渲染第 r 行：r=0 为表头。末列不补白。
    pub fn line(&self, r: usize) -> Option<String> {
        if self.cols == 0 || r > self.rows {
            return None;
        }
        let mut out = String::new();
        let mut c = 0usize;
        while c < self.cols {
            let text = match self.text_at(r, c) {
                Some(v) => v,
                None => return None,
            };
            let w = self.widths.get(c).copied().unwrap_or(0);
            if c + 1 == self.cols {
                out.push_str(text);
            } else if c == 0 || (self.cols == 7 && c == 1) {
                push_pad_left(&mut out, text, w);
                out.push(' ');
            } else {
                push_pad_right(&mut out, text, w);
                out.push(' ');
            }
            c += 1;
        }
        Some(out)
    }

    /// 整表渲染，逐行尾随换行。
    pub fn render(&self) -> String {
        let mut out = String::new();
        if self.cols == 0 {
            return out;
        }
        let mut r = 0usize;
        while r <= self.rows {
            if let Some(l) = self.line(r) {
                out.push_str(l.as_str());
            }
            out.push('\n');
            r += 1;
        }
        out
    }
}

/// 左对齐补白。
fn push_pad_left(out: &mut String, s: &str, w: usize) {
    out.push_str(s);
    let mut n = s.as_bytes().len();
    while n < w {
        out.push(' ');
        n += 1;
    }
}

/// 右对齐补白。
fn push_pad_right(out: &mut String, s: &str, w: usize) {
    let mut n = s.as_bytes().len();
    while n < w {
        out.push(' ');
        n += 1;
    }
    out.push_str(s);
}

// ---------------------------------------------------------------------------
// 表格装配
// ---------------------------------------------------------------------------

/// 按选项装配表格。
pub fn table(m: &Mounts, opts: &Options) -> Result<Table, u32> {
    if m.is_empty() {
        return Err(DF_ENOBS);
    }
    let mut t = Table::new();
    t.cols = opts.cols();
    t.set_head(0, "Filesystem")?;
    let mut c = 1usize;
    if opts.fstype {
        t.set_head(c, "Type")?;
        c += 1;
    }
    if opts.inode {
        t.set_head(c, "Inodes")?;
        t.set_head(c + 1, "IUsed")?;
        t.set_head(c + 2, "IFree")?;
        t.set_head(c + 3, "IUse%")?;
    } else {
        t.set_head(c, opts.unit.head())?;
        t.set_head(c + 1, "Used")?;
        t.set_head(c + 2, "Available")?;
        t.set_head(c + 3, "Use%")?;
    }
    c += 4;
    t.set_head(c, "Mounted on")?;

    let mut r = 0usize;
    let mut i = 0usize;
    while i < m.len() {
        let mt = match m.at(i) {
            Some(v) => v,
            None => return Err(DF_EVALUE),
        };
        i += 1;
        if !opts.all && mt.fs().pseudo() {
            continue;
        }
        if r >= TABLE_CAP {
            return Err(DF_ELEN);
        }
        t.set_cell(r, 0, mt.dev_text().as_str())?;
        let mut cc = 1usize;
        if opts.fstype {
            t.set_cell(r, cc, mt.fs().text())?;
            cc += 1;
        }
        if opts.inode {
            t.set_cell(r, cc, num_text(mt.inode_total()).as_str())?;
            t.set_cell(r, cc + 1, num_text(mt.inode_used()).as_str())?;
            t.set_cell(r, cc + 2, num_text(mt.inode_free()).as_str())?;
            t.set_cell(r, cc + 3, pct_text(mt.inode_pct()).as_str())?;
        } else {
            t.set_cell(r, cc, opts.unit.cell(mt.total()).as_str())?;
            t.set_cell(r, cc + 1, opts.unit.cell(mt.used()).as_str())?;
            t.set_cell(r, cc + 2, opts.unit.cell(mt.free()).as_str())?;
            t.set_cell(r, cc + 3, pct_text(mt.pct()).as_str())?;
        }
        t.set_cell(r, t.cols - 1, mt.dir_text().as_str())?;
        r += 1;
    }
    t.rows = r;
    if t.rows == 0 {
        return Err(DF_ENOBS);
    }
    t.measure();
    Ok(t)
}

/// 数字单元格文本。
fn num_text(v: u64) -> String {
    let mut s = String::new();
    push_u64(&mut s, v);
    s
}

/// 百分比单元格文本。
fn pct_text(p: u32) -> String {
    let mut s = String::new();
    push_u64(&mut s, p as u64);
    s.push('%');
    s
}

// ---------------------------------------------------------------------------
// 样本
// ---------------------------------------------------------------------------

/// 样本挂载表：四挂载点，其中 tmpfs 为伪文件系统。
pub fn sample_mounts() -> Result<Mounts, u32> {
    let mut m = Mounts::new();
    m.push(Mount::new(
        b"/dev/sda1",
        b"/",
        Fs::Ext4,
        1_000_000,
        350_000,
        65_536,
        12_000,
        4096,
    )?)?;
    m.push(Mount::new(
        b"/dev/sdb1",
        b"/boot",
        Fs::Xfs,
        250_000,
        40_000,
        128_000,
        512,
        4096,
    )?)?;
    m.push(Mount::new(b"tmpfs", b"/run", Fs::Tmpfs, 512_000, 96, 128_000, 40, 4096)?)?;
    m.push(Mount::new(
        b"/dev/mapper/vg-data",
        b"/srv/data",
        Fs::Btrfs,
        8_000_000,
        6_400_000,
        1_000_000,
        250_000,
        4096,
    )?)?;
    Ok(m)
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
            return Err(DF_ESLOT);
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
            return Err(DF_EBAL);
        }
        const VALS: [u32; 8] = [4, 9_762_000, 6_790_096, 35, 1, 4, 3, 7];
        const CODES: [u32; 6] = [DF_ECAP, DF_ENAME, DF_EOPT, DF_ESTATE, DF_ELEN, DF_ENOBS];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(DF_EBAL),
            };
            if s.slot as usize != i {
                return Err(DF_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(DF_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(DF_EVALUE);
                        }
                    }
                    None => return Err(DF_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(DF_EVALUE);
                        }
                    }
                    None => return Err(DF_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(DF_EBAL);
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
            return Err(DF_EBAL);
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
            Err(DF_ESEAL)
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

/// 账本组装：8 成 6 败，逐格对应 `audit` 的内置期望。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(count_cell(0, OP_MOUNT, sample_mounts().map(|m| m.len())));

    let _ = l.register(long_cell(1, OP_CAP, sample_mounts().map(|m| m.total_blocks())));

    let _ = l.register(long_cell(2, OP_CAP, sample_mounts().map(|m| m.total_used())));

    let _ = l.register(count_cell(
        3,
        OP_PCT,
        sample_mounts().and_then(|m| m.at(0).ok_or(DF_EVALUE)).map(|mt| mt.pct() as usize),
    ));

    let _ = l.register(count_cell(
        4,
        OP_PCT,
        sample_mounts().and_then(|m| m.at(2).ok_or(DF_EVALUE)).map(|mt| mt.pct() as usize),
    ));

    let _ = l.register(count_cell(5, OP_HUMAN, Ok(human(1_000_000).as_bytes().len())));

    let _ = l.register(count_cell(
        6,
        OP_TABLE,
        sample_mounts().and_then(|m| table(&m, &Options::new())).map(|t| t.rows()),
    ));

    let _ = l.register(count_cell(
        7,
        OP_TABLE,
        sample_mounts()
            .and_then(|m| {
                let mut o = Options::new();
                o.fstype = true;
                table(&m, &o)
            })
            .map(|t| t.cols()),
    ));

    let _ = l.register(long_cell(
        8,
        OP_MOUNT,
        Mount::new(b"/dev/sdz", b"/mnt", Fs::Ext4, 100, 200, 10, 1, 4096).map(|_| 0u64),
    ));

    let _ = l.register(long_cell(
        9,
        OP_MOUNT,
        Mount::new(b"", b"/mnt", Fs::Ext4, 100, 20, 10, 1, 4096).map(|_| 0u64),
    ));

    let _ = l.register(long_cell(10, OP_OPT, parse_argv(&[b"-z"]).map(|_| 0u64)));

    let _ = l.register(long_cell(11, OP_OPT, parse_argv(&[b"-k", b"-m"]).map(|_| 0u64)));

    let _ = l.register(long_cell(
        12,
        OP_MOUNT,
        Mount::new(&[b'q'; NAME_CAP + 1], b"/mnt", Fs::Ext4, 100, 20, 10, 1, 4096).map(|_| 0u64),
    ));

    let _ = l.register(long_cell(
        13,
        OP_TABLE,
        table(&Mounts::new(), &Options::new()).map(|_| 0u64),
    ));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 装载面。
fn mount_ok() -> bool {
    let m = match sample_mounts() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = m.len() == 4 && !m.is_empty();
    let b = match m.at(0) {
        Some(mt) => {
            mt.total() == 1_000_000
                && mt.used() == 350_000
                && mt.free() == 650_000
                && mt.pct() == 35
        }
        None => false,
    };
    let c = match m.at(3) {
        Some(mt) => mt.fs() == Fs::Btrfs && mt.dir() == b"/srv/data" && mt.block_size() == 4096,
        None => false,
    };
    let d = m.total_blocks() == 9_762_000 && m.total_used() == 6_790_096;
    let e = m.total_free() == m.total_blocks() - m.total_used();
    let f = m.visible(&Options::new()) == 3 && m.max_block_size() == 4096;
    a && b && c && d && e && f
}

/// 名称与字节闸门。
fn name_ok() -> bool {
    let a = Name::new(b"").err() == Some(DF_ENAME);
    let b = Name::new(&[0x01]).err() == Some(DF_EBYTE);
    let c = Name::new(&[b'q'; NAME_CAP + 1]).err() == Some(DF_ELEN);
    let d = Name::new(b"/dev/sda1").map(|n| n.len() == 9).unwrap_or(false);
    let e = match Mount::new(b"x", b"rel", Fs::Ext4, 10, 1, 10, 1, 4096) {
        Err(c) => c == DF_EMOUNT,
        Ok(_) => false,
    };
    let f = match sample_mounts() {
        Ok(mut m) => match Mount::new(b"/dev/sdq", b"/", Fs::Ext4, 10, 1, 10, 1, 4096) {
            Ok(mt) => m.push(mt).err() == Some(DF_EMOUNT),
            Err(_) => false,
        },
        Err(_) => false,
    };
    let g = match Name::new(b"/run") {
        Ok(n) => n.named(b"/run") && !n.named(b"/ru") && n.first() == Some(b'/'),
        Err(_) => false,
    };
    a && b && c && d && e && f && g
}

/// 容量与类型闸门。
fn cap_ok() -> bool {
    let a = Mount::new(b"/dev/sdq", b"/mnt", Fs::Ext4, 100, 200, 10, 1, 4096).err()
        == Some(DF_ECAP);
    let b = Mount::new(b"/dev/sdq", b"/mnt", Fs::Ext4, 100, 20, 10, 11, 4096).err()
        == Some(DF_ECAP);
    let c = Mount::new(b"/dev/sdq", b"/mnt", Fs::Ext4, 0, 0, 10, 1, 4096).err() == Some(DF_ECAP);
    let d = Mount::new(b"/dev/sdq", b"/mnt", Fs::Ext4, 100, 20, 0, 0, 4096).err() == Some(DF_ECAP);
    let e = Mount::new(b"/dev/sdq", b"/mnt", Fs::Ext4, 100, 20, 10, 1, 0).err() == Some(DF_ETYPE);
    let f = Mount::new(b"/dev/sdq", b"/mnt", Fs::Ext4, 100, 100, 10, 10, 4096).is_ok();
    a && b && c && d && e && f
}

/// 百分比进一面。
fn pct_ok() -> bool {
    let m = match sample_mounts() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = match (m.at(0), m.at(1), m.at(2), m.at(3)) {
        (Some(a), Some(b), Some(c), Some(d)) => {
            a.pct() == 35 && b.pct() == 16 && c.pct() == 1 && d.pct() == 80
        }
        _ => false,
    };
    let b = match (m.at(0), m.at(1), m.at(3)) {
        (Some(a), Some(b), Some(d)) => {
            a.inode_pct() == 19 && b.inode_pct() == 1 && d.inode_pct() == 25
        }
        _ => false,
    };
    let c = percent(0, 100) == 0 && percent(1, 100_000) == 1 && percent(100, 100) == 100;
    let d = percent(0, 0) == 0 && percent(1, 0) == 0;
    let e = match m.at(0) {
        Some(mt) => mt.inode_free() == 53_536 && mt.bytes() == 4_096_000_000,
        None => false,
    };
    a && b && c && d && e
}

/// 人读面。
fn human_ok() -> bool {
    human(96) == "96K"
        && human(1024) == "1.0M"
        && human(1_000_000) == "977M"
        && human(8_000_000) == "7.7G"
        && human(1_600_000) == "1.6G"
        && human(512_000) == "500M"
        && human(250_000) == "245M"
        && human(0) == "0K"
}

/// 选项面。
fn option_ok() -> bool {
    let a = match parse_argv(&[b"-h"]) {
        Ok((o, _, n)) => o.unit == Unit::Human && n == 0,
        Err(_) => false,
    };
    let b = match parse_argv(&[b"-hi"]) {
        Ok((o, _, _)) => o.unit == Unit::Human && o.inode,
        Err(_) => false,
    };
    let c = match parse_argv(&[b"-T", b"-a"]) {
        Ok((o, _, _)) => o.fstype && o.all && o.cols() == 7,
        Err(_) => false,
    };
    let d = match parse_argv(&[b"-m", b"/"]) {
        Ok((o, names, n)) => o.unit == Unit::Mega && n == 1 && names[0].named(b"/"),
        Err(_) => false,
    };
    let e = match parse_argv(&[b"-k"]) {
        Ok((o, _, _)) => o.unit == Unit::Kilo,
        Err(_) => false,
    };
    a && b && c && d && e
}

/// 选项错误码面。
fn option_err_ok() -> bool {
    let a = parse_argv(&[b"-z"]).err() == Some(DF_EOPT);
    let b = parse_argv(&[b"--help"]).err() == Some(DF_EOPT);
    let c = parse_argv(&[b"-k", b"-m"]).err() == Some(DF_ESTATE);
    let d = parse_argv(&[b"-h", b"-k"]).err() == Some(DF_ESTATE);
    let e = parse_argv(&[b"-"]).err() == Some(DF_EBYTE);
    let f = parse_argv(&[b"\x01"]).err() == Some(DF_EBYTE);
    let g = parse_argv(&[b"p1", b"p2", b"p3", b"p4", b"p5"]).err() == Some(DF_ELEN);
    a && b && c && d && e && f && g
}

/// 六列缺省表面。
fn default_ok() -> bool {
    let m = match sample_mounts() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let t = match table(&m, &Options::new()) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = t.cols() == 6 && t.rows() == 3 && !t.is_empty();
    let b = t.head(0) == Some("Filesystem")
        && t.head(1) == Some("1K-blocks")
        && t.head(2) == Some("Used")
        && t.head(3) == Some("Available")
        && t.head(4) == Some("Use%")
        && t.head(5) == Some("Mounted on")
        && t.head(6).is_none();
    let c = t.width(0) == Some(19)
        && t.width(1) == Some(9)
        && t.width(2) == Some(7)
        && t.width(3) == Some(9)
        && t.width(4) == Some(4)
        && t.width(5) == Some(10)
        && t.width(6).is_none();
    let d = t.cell(0, 0) == Some("/dev/sda1")
        && t.cell(0, 1) == Some("1000000")
        && t.cell(0, 2) == Some("350000")
        && t.cell(0, 3) == Some("650000")
        && t.cell(0, 4) == Some("35%")
        && t.cell(0, 5) == Some("/")
        && t.cell(3, 0).is_none();
    let e = t.cell(2, 0) == Some("/dev/mapper/vg-data")
        && t.cell(2, 5) == Some("/srv/data")
        && t.cell(2, 4) == Some("80%");
    let f = t.render().lines().count() == t.rows() + 1;
    a && b && c && d && e && f
}

/// inode 档面。
fn inode_ok() -> bool {
    let m = match sample_mounts() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let mut o = Options::new();
    o.inode = true;
    let t = match table(&m, &o) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = t.cols() == 6 && t.rows() == 3;
    let b = t.head(0) == Some("Filesystem")
        && t.head(1) == Some("Inodes")
        && t.head(2) == Some("IUsed")
        && t.head(3) == Some("IFree")
        && t.head(4) == Some("IUse%")
        && t.head(5) == Some("Mounted on");
    let c = t.cell(0, 1) == Some("65536")
        && t.cell(0, 2) == Some("12000")
        && t.cell(0, 3) == Some("53536")
        && t.cell(0, 4) == Some("19%");
    let d = t.cell(1, 4) == Some("1%") && t.cell(2, 4) == Some("25%");
    a && b && c && d
}

/// 型列档面。
fn fstype_ok() -> bool {
    let m = match sample_mounts() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let mut o = Options::new();
    o.fstype = true;
    let t = match table(&m, &o) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = t.cols() == 7 && t.rows() == 3;
    let b = t.head(1) == Some("Type") && t.head(2) == Some("1K-blocks") && t.head(6) == Some("Mounted on");
    let c = t.cell(0, 1) == Some("ext4")
        && t.cell(1, 1) == Some("xfs")
        && t.cell(2, 1) == Some("btrfs");
    let d = t.cell(2, 6) == Some("/srv/data") && t.width(1) == Some(5);
    let mut o2 = Options::new();
    o2.fstype = true;
    o2.inode = true;
    let t2 = match table(&m, &o2) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let e = t2.cols() == 7 && t2.head(1) == Some("Type") && t2.head(2) == Some("Inodes");
    a && b && c && d && e
}

/// 单位与全出面。
fn unit_table_ok() -> bool {
    let m = match sample_mounts() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let mut h = Options::new();
    h.unit = Unit::Human;
    let th = match table(&m, &h) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = th.head(1) == Some("Size")
        && th.cell(0, 1) == Some("977M")
        && th.cell(0, 2) == Some("342M")
        && th.cell(0, 3) == Some("635M")
        && th.cell(0, 4) == Some("35%");
    let b = th.cell(1, 1) == Some("245M") && th.cell(1, 2) == Some("40M");
    let c = th.cell(2, 1) == Some("7.7G") && th.cell(2, 2) == Some("6.2G") && th.cell(2, 3) == Some("1.6G");
    let mut mg = Options::new();
    mg.unit = Unit::Mega;
    let tm = match table(&m, &mg) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let d = tm.head(1) == Some("1M-blocks")
        && tm.cell(0, 1) == Some("977")
        && tm.cell(0, 2) == Some("342")
        && tm.cell(2, 1) == Some("7813")
        && tm.cell(2, 3) == Some("1563");
    let mut al = Options::new();
    al.all = true;
    let ta = match table(&m, &al) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let e = ta.rows() == 4 && ta.cell(2, 0) == Some("tmpfs") && ta.cell(2, 5) == Some("/run");
    a && b && c && d && e
}

/// 路径归属面。
fn select_ok() -> bool {
    let m = match sample_mounts() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = m.resolve(b"/srv/data/x") == Some(3);
    let b = m.resolve(b"/boot") == Some(1);
    let c = m.resolve(b"/run/x") == Some(2);
    let d = m.resolve(b"none") == None;
    let e = m.resolve(b"/") == Some(0);
    let f = m.resolve(b"/boot/grub/x") == Some(1);
    let g = match Mounts::new().resolve(b"/") {
        None => true,
        Some(_) => false,
    };
    let h = m.dev_count(b"/dev/sda1") == 1 && m.dev_count(b"none") == 0;
    a && b && c && d && e && f && g && h
}

/// 表闸门面。
fn table_gate_ok() -> bool {
    let empty = Table::new();
    let a = empty.cols() == 0 && empty.rows() == 0 && empty.render() == "" && empty.is_empty();
    let b = empty.head(0).is_none() && empty.cell(0, 0).is_none() && empty.line(0).is_none();
    let c = table(&Mounts::new(), &Options::new()).err() == Some(DF_ENOBS);
    let d = match sample_mounts() {
        Ok(_) => {
            let mut o = Options::new();
            o.all = false;
            // 只装伪文件系统的表在缺省档里没有可见行。
            let mut p = Mounts::new();
            let ok = match Mount::new(b"tmpfs", b"/run", Fs::Tmpfs, 512_000, 96, 128_000, 40, 4096) {
                Ok(mt) => p.push(mt).is_ok(),
                Err(_) => false,
            };
            ok && table(&p, &o).err() == Some(DF_ENOBS)
                && table(&p, &o2_all()).map(|t| t.rows()) == Ok(1)
        }
        Err(_) => false,
    };
    let e = match sample_mounts() {
        Ok(v) => {
            let mut o = Options::new();
            o.all = true;
            match table(&v, &o) {
                Ok(t) => t.line(0).is_some() && t.line(5).is_none() && t.cell(0, 6).is_none(),
                Err(_) => false,
            }
        }
        Err(_) => false,
    };
    a && b && c && d && e
}

/// 全出档。
fn o2_all() -> Options {
    let mut o = Options::new();
    o.all = true;
    o
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = mount_ok() && name_ok();
    out[1] = cap_ok() && pct_ok() && human_ok();
    out[2] = option_ok() && option_err_ok();
    out[3] = default_ok() && inode_ok() && fstype_ok() && unit_table_ok() && select_ok()
        && table_gate_ok()
        && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11545_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11545.df");

    s.add(
        "DF01-挂载装载",
        {
            mount_ok()
        },
        "static",
    );

    s.add(
        "DF02-名称与挂载点",
        {
            name_ok()
        },
        "static",
    );

    s.add(
        "DF03-容量与类型",
        {
            cap_ok()
        },
        "static",
    );

    s.add(
        "DF04-百分比进一",
        {
            pct_ok()
        },
        "static",
    );

    s.add(
        "DF05-人读单位",
        {
            human_ok()
        },
        "static",
    );

    s.add(
        "DF06-六列缺省表",
        {
            default_ok()
        },
        "static",
    );

    s.add(
        "DF07-inode 档",
        {
            inode_ok()
        },
        "static",
    );

    s.add(
        "DF08-型列档",
        {
            fstype_ok()
        },
        "static",
    );

    s.add(
        "DF09-选项组合",
        {
            option_ok() && option_err_ok()
        },
        "static",
    );

    s.add(
        "DF10-单位与全出",
        {
            unit_table_ok()
        },
        "static",
    );

    s.add(
        "DF11-账本对账",
        {
            build_ledger().audit() == Ok(())
        },
        "static",
    );

    s.add(
        "DF12-路径归属与空表",
        {
            select_ok() && table_gate_ok()
        },
        "static",
    );

    s.add(
        "DF13-冻结重放四注入",
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
    fn mounts_shape() {
        let m = sample_mounts().expect("m");
        assert_eq!(m.len(), 4);
        assert_eq!(m.visible(&Options::new()), 3);
        assert_eq!(m.total_blocks(), 9_762_000);
        assert_eq!(m.total_used(), 6_790_096);
        assert_eq!(m.total_free(), 2_971_904);
        assert_eq!(m.max_block_size(), 4096);
    }

    #[test]
    fn mount_gates() {
        assert_eq!(Mount::new(b"", b"/mnt", Fs::Ext4, 1, 0, 1, 0, 4096).err(), Some(DF_ENAME));
        assert_eq!(Mount::new(b"d", b"rel", Fs::Ext4, 1, 0, 1, 0, 4096).err(), Some(DF_EMOUNT));
        assert_eq!(Name::new(&[0x7f]).err(), Some(DF_EBYTE));
        assert_eq!(Name::new(&[b'q'; NAME_CAP + 1]).err(), Some(DF_ELEN));
        let mut m = sample_mounts().expect("m");
        let dup = Mount::new(b"/dev/sdq", b"/srv/data", Fs::Ext4, 1, 0, 1, 0, 4096);
        assert_eq!(m.push(dup.expect("d")), Err(DF_EMOUNT));
    }

    #[test]
    fn capacities() {
        let m = sample_mounts().expect("m");
        let a = m.at(0).expect("a");
        assert_eq!(a.free(), 650_000);
        assert_eq!(a.inode_free(), 53_536);
        assert_eq!(a.bytes(), 4_096_000_000);
        assert_eq!(a.pct(), 35);
        assert_eq!(a.inode_pct(), 19);
        assert_eq!(percent(0, 0), 0);
        assert_eq!(percent(1, 3), 34);
        assert_eq!(percent(3, 3), 100);
    }

    #[test]
    fn human_units() {
        assert_eq!(human(0).as_str(), "0K");
        assert_eq!(human(96).as_str(), "96K");
        assert_eq!(human(1024).as_str(), "1.0M");
        assert_eq!(human(1_000_000).as_str(), "977M");
        assert_eq!(human(8_000_000).as_str(), "7.7G");
        assert_eq!(human(1_600_000).as_str(), "1.6G");
    }

    #[test]
    fn options() {
        assert_eq!(parse_argv(&[b"-h"]).map(|o| o.0.unit), Ok(Unit::Human));
        assert_eq!(parse_argv(&[b"-T"]).map(|o| o.0.fstype), Ok(true));
        assert_eq!(parse_argv(&[b"-a"]).map(|o| o.0.all), Ok(true));
        assert_eq!(parse_argv(&[b"-i"]).map(|o| o.0.inode), Ok(true));
        assert_eq!(parse_argv(&[b"-hi"]).map(|o| o.0.inode), Ok(true));
        assert_eq!(parse_argv(&[b"-"]).err(), Some(DF_EBYTE));
        assert_eq!(parse_argv(&[b"-z"]).err(), Some(DF_EOPT));
        assert_eq!(parse_argv(&[b"-k", b"-m"]).err(), Some(DF_ESTATE));
        assert_eq!(parse_argv(&[b"a", b"b", b"c", b"d", b"e"]).err(), Some(DF_ELEN));
    }

    #[test]
    fn default_table() {
        let m = sample_mounts().expect("m");
        let t = table(&m, &Options::new()).expect("t");
        assert_eq!(t.cols(), 6);
        assert_eq!(t.rows(), 3);
        assert_eq!(t.width(0), Some(19));
        assert_eq!(t.width(1), Some(9));
        assert_eq!(t.width(2), Some(7));
        assert_eq!(t.width(4), Some(4));
        assert_eq!(t.head(1), Some("1K-blocks"));
        assert_eq!(t.cell(0, 0), Some("/dev/sda1"));
        assert_eq!(t.cell(0, 4), Some("35%"));
        assert_eq!(t.cell(1, 5), Some("/boot"));
        assert_eq!(t.cell(2, 0), Some("/dev/mapper/vg-data"));
        assert_eq!(t.cell(3, 0), None);
        let rendered = t.render();
        assert_eq!(rendered.lines().count(), 4);
        assert!(rendered.starts_with("Filesystem "));
    }

    #[test]
    fn inode_table() {
        let m = sample_mounts().expect("m");
        let mut o = Options::new();
        o.inode = true;
        let t = table(&m, &o).expect("t");
        assert_eq!(t.head(1), Some("Inodes"));
        assert_eq!(t.head(4), Some("IUse%"));
        assert_eq!(t.cell(0, 1), Some("65536"));
        assert_eq!(t.cell(0, 3), Some("53536"));
        assert_eq!(t.cell(0, 4), Some("19%"));
        assert_eq!(t.cell(1, 4), Some("1%"));
    }

    #[test]
    fn type_table() {
        let m = sample_mounts().expect("m");
        let mut o = Options::new();
        o.fstype = true;
        let t = table(&m, &o).expect("t");
        assert_eq!(t.cols(), 7);
        assert_eq!(t.head(1), Some("Type"));
        assert_eq!(t.head(2), Some("1K-blocks"));
        assert_eq!(t.cell(0, 1), Some("ext4"));
        assert_eq!(t.cell(2, 1), Some("btrfs"));
        assert_eq!(t.cell(2, 6), Some("/srv/data"));
        assert_eq!(t.width(1), Some(5));
    }

    #[test]
    fn human_table() {
        let m = sample_mounts().expect("m");
        let mut o = Options::new();
        o.unit = Unit::Human;
        let t = table(&m, &o).expect("t");
        assert_eq!(t.head(1), Some("Size"));
        assert_eq!(t.cell(0, 1), Some("977M"));
        assert_eq!(t.cell(0, 2), Some("342M"));
        assert_eq!(t.cell(0, 3), Some("635M"));
        assert_eq!(t.cell(2, 1), Some("7.7G"));
        assert_eq!(t.cell(2, 3), Some("1.6G"));
    }

    #[test]
    fn mega_table() {
        let m = sample_mounts().expect("m");
        let mut o = Options::new();
        o.unit = Unit::Mega;
        let t = table(&m, &o).expect("t");
        assert_eq!(t.head(1), Some("1M-blocks"));
        assert_eq!(t.cell(0, 1), Some("977"));
        assert_eq!(t.cell(2, 1), Some("7813"));
        assert_eq!(t.cell(2, 3), Some("1563"));
    }

    #[test]
    fn all_table() {
        let m = sample_mounts().expect("m");
        let mut o = Options::new();
        o.all = true;
        let t = table(&m, &o).expect("t");
        assert_eq!(t.rows(), 4);
        assert_eq!(t.cell(2, 0), Some("tmpfs"));
        assert_eq!(t.cell(2, 5), Some("/run"));
    }

    #[test]
    fn paths() {
        let m = sample_mounts().expect("m");
        assert_eq!(m.resolve(b"/srv/data/x"), Some(3));
        assert_eq!(m.resolve(b"/boot/grub"), Some(1));
        assert_eq!(m.resolve(b"/"), Some(0));
        assert_eq!(m.resolve(b"none"), None);
        assert_eq!(m.resolve(b"/none"), Some(0));
        assert_eq!(Mounts::new().resolve(b"/"), None);
    }

    #[test]
    fn table_errors() {
        assert_eq!(table(&Mounts::new(), &Options::new()).err(), Some(DF_ENOBS));
        let mut p = Mounts::new();
        p.push(Mount::new(b"tmpfs", b"/run", Fs::Tmpfs, 512_000, 96, 128_000, 40, 4096).expect("p"))
            .expect("push");
        assert_eq!(table(&p, &Options::new()).err(), Some(DF_ENOBS));
        let mut o = Options::new();
        o.all = true;
        assert_eq!(table(&p, &o).map(|t| t.rows()), Ok(1));
    }

    #[test]
    fn empty_table() {
        let t = Table::new();
        assert_eq!(t.cols(), 0);
        assert_eq!(t.render().as_str(), "");
        assert_eq!(t.head(0), None);
        assert_eq!(t.line(0), None);
        assert_eq!(t.cell(0, 0), None);
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.sample(0).map(|s| s.value), Some(4));
        assert_eq!(l.sample(13).map(|s| s.code), Some(DF_ENOBS));
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        assert!(!l.is_frozen());
        let root = l.freeze().expect("freeze");
        assert!(l.is_frozen());
        assert_eq!(l.root(), root);
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze().err(), Some(DF_EBAL));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn injections() {
        let four = inject_four();
        assert!(four[0] && four[1] && four[2] && four[3]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11545_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }
}
