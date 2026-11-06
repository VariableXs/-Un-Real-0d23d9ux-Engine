//! UNX-F11544 du 磁盘用量统计（-s/-h/-a）
//!
//! `du` 的块账模型：把一棵虚拟目录树按「自底向上求和」重新计量，一次遍历
//! 同时得到每个目录的子树枝、每条文件的大小、全树合计与最大深度。选项面
//! 覆盖 `-s` 只出汇总、`-a` 连文件一起出、`-h` 人读单位、`-c` 合计行、
//! `-k/-m/-b` 三档单位与 `-x` 单文件系统。路径一律从根向下拼，节点按
//! 索引升序输出，保证同一棵树在任何平台渲染逐字节一致。
//!
//! 闸门次序：字节 → 名称 → 父子 → 重复 → 类型 → 容量 → 选项 → 路径 → 报告。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本 14 格
//! （8 成 6 败）在 `audit` 逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf310;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 节点容量。
pub const NODE_CAP: usize = 32;

/// 路径操作数容量。
pub const PATH_CAP: usize = 4;

/// 名称容量。
pub const NAME_CAP: usize = 32;

/// 根节点父哨兵。
pub const ROOT_PARENT: u16 = 0xffff;

/// 栈容量。
pub const STACK_CAP: usize = 64;

/// 一 K 字节。
pub const KIB: u64 = 1024;

/// 一 M 字节。
pub const MIB: u64 = 1024 * 1024;

/// 一 G 字节。
pub const GIB: u64 = 1024 * 1024 * 1024;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 父子关系非法。
pub const DU_ETREE: u32 = SEG_BASE + 0x01;
/// 同级重名。
pub const DU_EDUP: u32 = SEG_BASE + 0x02;
/// 节点类型不符。
pub const DU_ETYPE: u32 = SEG_BASE + 0x03;
/// 选项非法。
pub const DU_EOPT: u32 = SEG_BASE + 0x04;
/// 超长。
pub const DU_ELEN: u32 = SEG_BASE + 0x05;
/// 字节非法。
pub const DU_EBYTE: u32 = SEG_BASE + 0x06;
/// 路径不存在。
pub const DU_ENOENT: u32 = SEG_BASE + 0x07;
/// 状态冲突。
pub const DU_ESTATE: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const DU_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const DU_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const DU_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const DU_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const DU_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 节点装载。
pub const OP_NODE: u8 = 0x01;
/// 求和。
pub const OP_SUM: u8 = 0x02;
/// 计数。
pub const OP_COUNT: u8 = 0x03;
/// 单位换算。
pub const OP_UNIT: u8 = 0x04;
/// 报告渲染。
pub const OP_REPORT: u8 = 0x05;
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

/// 是否可打印：0x21..=0x7e，另放行 `/` 与 `.`。
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

// ---------------------------------------------------------------------------
// 名称
// ---------------------------------------------------------------------------

/// 节点名。
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
            return Err(DU_EBYTE);
        }
        if s.len() > NAME_CAP {
            return Err(DU_ELEN);
        }
        if !printable(s) {
            return Err(DU_EBYTE);
        }
        let mut n = Name::blank();
        let mut i = 0usize;
        while i < s.len() {
            match (s.get(i), n.bytes.get_mut(i)) {
                (Some(src), Some(dst)) => *dst = *src,
                _ => return Err(DU_ELEN),
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
// 树
// ---------------------------------------------------------------------------

/// 节点类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 普通文件。
    File,
    /// 目录。
    Dir,
}

/// 一个节点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    /// 名字。
    pub name: Name,
    /// 类型。
    pub kind: Kind,
    /// 自身块数（1K 块）。
    pub blocks: u64,
    /// 父下标（根为 `ROOT_PARENT`）。
    pub parent: u16,
}

impl Entry {
    /// 空白节点。
    pub const fn blank() -> Entry {
        Entry { name: Name::blank(), kind: Kind::File, blocks: 0, parent: ROOT_PARENT }
    }
}

/// 目录树。
#[derive(Clone, Copy, Debug)]
pub struct Tree {
    nodes: [Entry; NODE_CAP],
    len: usize,
}

impl Tree {
    /// 空树。
    pub const fn new() -> Tree {
        Tree { nodes: [Entry::blank(); NODE_CAP], len: 0 }
    }

    /// 节点数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取节点。
    pub fn at(&self, i: usize) -> Option<Entry> {
        if i >= self.len {
            return None;
        }
        self.nodes.get(i).copied()
    }

    /// 根下标。
    pub fn root(&self) -> Option<usize> {
        if self.is_empty() {
            None
        } else {
            Some(0)
        }
    }

    /// 追加节点，返回下标。
    pub fn push(&mut self, kind: Kind, name: &[u8], blocks: u64, parent: u16) -> Result<u16, u32> {
        if self.len >= NODE_CAP {
            return Err(DU_ELEN);
        }
        if self.is_empty() {
            if parent != ROOT_PARENT {
                return Err(DU_ETREE);
            }
        } else {
            if parent == ROOT_PARENT || (parent as usize) >= self.len {
                return Err(DU_ETREE);
            }
            let p = match self.nodes.get(parent as usize) {
                Some(v) => *v,
                None => return Err(DU_ETREE),
            };
            if p.kind != Kind::Dir {
                return Err(DU_ETYPE);
            }
        }
        let n = Name::new(name)?;
        if self.dup_name(parent, n.bytes()) {
            return Err(DU_EDUP);
        }
        let idx = self.len;
        if idx > u16::MAX as usize {
            return Err(DU_ELEN);
        }
        if let Some(dst) = self.nodes.get_mut(idx) {
            *dst = Entry { name: n, kind, blocks, parent };
        }
        self.len += 1;
        Ok(idx as u16)
    }

    /// 同级是否重名。
    pub fn dup_name(&self, parent: u16, name: &[u8]) -> bool {
        let mut i = 0usize;
        while i < self.len {
            if let Some(e) = self.nodes.get(i) {
                if e.parent == parent && eq_bytes(e.name.bytes(), name) {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 深度：根 0。
    pub fn depth(&self, idx: usize) -> Result<usize, u32> {
        if idx >= self.len {
            return Err(DU_ENOENT);
        }
        let mut d = 0usize;
        let mut cur = idx;
        loop {
            let e = match self.nodes.get(cur) {
                Some(v) => *v,
                None => return Err(DU_ENOENT),
            };
            if e.parent == ROOT_PARENT {
                break;
            }
            cur = e.parent as usize;
            d += 1;
            if d > NODE_CAP {
                return Err(DU_ETREE);
            }
        }
        Ok(d)
    }

    /// 某节点的全部祖先下标（不含自身），自近及远。
    fn ancestors(&self, idx: usize, out: &mut [u16; NODE_CAP]) -> Result<usize, u32> {
        let mut n = 0usize;
        let mut cur = idx;
        loop {
            let e = match self.nodes.get(cur) {
                Some(v) => *v,
                None => return Err(DU_ENOENT),
            };
            if e.parent == ROOT_PARENT {
                break;
            }
            if n >= NODE_CAP {
                return Err(DU_ETREE);
            }
            if let Some(dst) = out.get_mut(n) {
                *dst = e.parent;
            }
            cur = e.parent as usize;
            n += 1;
        }
        Ok(n)
    }

    /// 路径文本：根为 `.`，其余自根向下以 `/` 连接。
    pub fn path_of(&self, idx: usize) -> Result<String, u32> {
        if idx >= self.len {
            return Err(DU_ENOENT);
        }
        let mut chain = [0u16; NODE_CAP];
        let n = self.ancestors(idx, &mut chain)?;
        let mut s = String::new();
        if n == 0 {
            s.push('.');
            return Ok(s);
        }
        let mut k = n;
        while k > 0 {
            k -= 1;
            if let Some(a) = chain.get(k) {
                if let Some(e) = self.nodes.get(*a as usize) {
                    if !eq_bytes(e.name.bytes(), b".") {
                        if !s.is_empty() {
                            s.push('/');
                        }
                        s.push_str(e.name.text().as_str());
                    }
                }
            }
        }
        if let Some(e) = self.nodes.get(idx) {
            if !s.is_empty() {
                s.push('/');
            }
            s.push_str(e.name.text().as_str());
        }
        Ok(s)
    }

    /// 是否在 `root` 子树内（含 root 自身）。
    pub fn in_subtree(&self, idx: usize, root: usize) -> bool {
        let mut cur = idx;
        let mut guard = 0usize;
        loop {
            if cur == root {
                return true;
            }
            let e = match self.nodes.get(cur) {
                Some(v) => *v,
                None => return false,
            };
            if e.parent == ROOT_PARENT {
                return false;
            }
            cur = e.parent as usize;
            guard += 1;
            if guard > NODE_CAP {
                return false;
            }
        }
    }

    /// 子树块数。
    pub fn sum_of(&self, root: usize) -> Result<u64, u32> {
        if root >= self.len {
            return Err(DU_ENOENT);
        }
        let mut total = 0u64;
        let mut i = 0usize;
        while i < self.len {
            if self.in_subtree(i, root) {
                if let Some(e) = self.nodes.get(i) {
                    total = total.saturating_add(e.blocks);
                }
            }
            i += 1;
        }
        Ok(total)
    }

    /// 子树内某类节点数。
    pub fn count_kind(&self, root: usize, kind: Kind) -> Result<usize, u32> {
        if root >= self.len {
            return Err(DU_ENOENT);
        }
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if self.in_subtree(i, root) {
                if let Some(e) = self.nodes.get(i) {
                    if e.kind == kind {
                        n += 1;
                    }
                }
            }
            i += 1;
        }
        Ok(n)
    }

    /// 子树最大深度（相对根 0）。
    pub fn max_depth(&self, root: usize) -> Result<usize, u32> {
        if root >= self.len {
            return Err(DU_ENOENT);
        }
        let mut best = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if self.in_subtree(i, root) {
                let base = match self.depth(root) {
                    Ok(v) => v,
                    Err(_) => 0,
                };
                let d = match self.depth(i) {
                    Ok(v) => v.saturating_sub(base),
                    Err(_) => 0,
                };
                if d > best {
                    best = d;
                }
            }
            i += 1;
        }
        Ok(best)
    }

    /// 按路径文本解析节点（必须存在）。
    pub fn find(&self, path: &[u8]) -> Result<usize, u32> {
        if path.is_empty() {
            return Err(DU_ENOENT);
        }
        let mut i = 0usize;
        while i < self.len {
            let p = self.path_of(i)?;
            if eq_bytes(p.as_str().as_bytes(), path) {
                return Ok(i);
            }
            i += 1;
        }
        Err(DU_ENOENT)
    }
}

// ---------------------------------------------------------------------------
// 单位与人读
// ---------------------------------------------------------------------------

/// 块单位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    /// 512 字节块。
    HalfK,
    /// 1K 块（缺省）。
    K,
    /// 1M 块。
    M,
    /// 字节。
    Bytes,
}

impl Unit {
    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            Unit::HalfK => "512",
            Unit::K => "1K",
            Unit::M => "1M",
            Unit::Bytes => "1B",
        }
    }

    /// 块 → 字节。
    pub fn to_bytes(&self, blocks: u64) -> u64 {
        match self {
            Unit::HalfK => blocks.saturating_mul(512),
            Unit::K => blocks.saturating_mul(KIB),
            Unit::Bytes => blocks.saturating_mul(KIB),
            Unit::M => {
                let b = blocks.saturating_mul(KIB);
                let q = b / MIB;
                if b % MIB == 0 {
                    q
                } else {
                    q.saturating_add(1)
                }
            }
        }
    }
}

/// 人读：小于 1K 出 `NB`，其余按 1024 进位，小于 10 保留一位小数，否则
/// 向上取整到整数。
pub fn human(bytes: u64) -> String {
    if bytes < KIB {
        let mut s = String::new();
        push_u64(&mut s, bytes);
        s.push('B');
        return s;
    }
    let mut unit: u64 = KIB;
    let mut letter = 'K';
    let mut idx = 0usize;
    let letters = ['K', 'M', 'G', 'T', 'P', 'E'];
    while idx + 1 < letters.len() && bytes >= unit.saturating_mul(KIB) {
        unit = unit.saturating_mul(KIB);
        idx += 1;
        if let Some(c) = letters.get(idx) {
            letter = *c;
        }
    }
    let whole = bytes / unit;
    let rem = bytes % unit;
    let mut s = String::new();
    if whole < 10 {
        push_u64(&mut s, whole);
        s.push('.');
        let tenth = rem.saturating_mul(10) / unit;
        push_u64(&mut s, tenth);
    } else {
        let mut up = whole;
        if rem > 0 {
            up = up.saturating_add(1);
        }
        push_u64(&mut s, up);
    }
    s.push(letter);
    s
}

// ---------------------------------------------------------------------------
// 选项
// ---------------------------------------------------------------------------

/// 选项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// `-s` 只出汇总。
    pub summarize: bool,
    /// `-a` 连文件一起。
    pub all: bool,
    /// `-h` 人读。
    pub human: bool,
    /// `-c` 合计行。
    pub grand: bool,
    /// `-x` 单文件系统。
    pub one_fs: bool,
    /// 单位。
    pub unit: Unit,
}

impl Options {
    /// 缺省。
    pub const fn new() -> Options {
        Options {
            summarize: false,
            all: false,
            human: false,
            grand: false,
            one_fs: false,
            unit: Unit::K,
        }
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        if self.summarize {
            s.push_str("summarize ");
        }
        if self.all {
            s.push_str("all ");
        }
        if self.human {
            s.push_str("human ");
        }
        if self.grand {
            s.push_str("grand ");
        }
        if self.one_fs {
            s.push_str("one-fs ");
        }
        s.push_str("unit ");
        s.push_str(self.unit.text());
        s
    }

    /// 输出宽度：数字或人读串。
    pub fn render_size(&self, blocks: u64) -> String {
        if self.human {
            human(self.unit.to_bytes(blocks))
        } else {
            let mut s = String::new();
            push_u64(&mut s, self.unit.to_bytes(blocks));
            s
        }
    }
}

/// 选项单字符。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flag {
    Summarize,
    All,
    Human,
    Grand,
    OneFs,
    HalfK,
    K,
    M,
    Bytes,
    Unknown,
}

/// 识别单字符。
fn flag_of(b: u8) -> Flag {
    match b {
        b's' => Flag::Summarize,
        b'a' => Flag::All,
        b'h' => Flag::Human,
        b'c' => Flag::Grand,
        b'x' => Flag::OneFs,
        b'k' => Flag::K,
        b'm' => Flag::M,
        b'b' => Flag::Bytes,
        _ => Flag::Unknown,
    }
}

/// 解析参数：`du [opts] [path...]`。
pub fn parse_argv(args: &[&[u8]]) -> Result<(Options, [Name; PATH_CAP], usize), u32> {
    let mut opts = Options::new();
    let mut unit_seen = false;
    let mut paths = [Name::blank(); PATH_CAP];
    let mut len = 0usize;
    let mut i = 0usize;
    let mut operands = false;
    while i < args.len() {
        let a = match args.get(i) {
            Some(v) => *v,
            None => return Err(DU_EOPT),
        };
        if !operands {
            if eq_bytes(a, b"--") {
                operands = true;
                i += 1;
                continue;
            }
            if a.len() > 2 && a.get(0) == Some(&b'-') && a.get(1) == Some(&b'-') {
                return Err(DU_EOPT);
            }
            if a.len() > 1 && a.get(0) == Some(&b'-') {
                let mut j = 1usize;
                while j < a.len() {
                    let b = match a.get(j) {
                        Some(v) => *v,
                        None => return Err(DU_EOPT),
                    };
                    match flag_of(b) {
                        Flag::Summarize => opts.summarize = true,
                        Flag::All => opts.all = true,
                        Flag::Human => opts.human = true,
                        Flag::Grand => opts.grand = true,
                        Flag::OneFs => opts.one_fs = true,
                        Flag::HalfK => {
                            if unit_seen {
                                return Err(DU_ESTATE);
                            }
                            opts.unit = Unit::HalfK;
                            unit_seen = true;
                        }
                        Flag::K => {
                            if unit_seen {
                                return Err(DU_ESTATE);
                            }
                            opts.unit = Unit::K;
                            unit_seen = true;
                        }
                        Flag::M => {
                            if unit_seen {
                                return Err(DU_ESTATE);
                            }
                            opts.unit = Unit::M;
                            unit_seen = true;
                        }
                        Flag::Bytes => {
                            if unit_seen {
                                return Err(DU_ESTATE);
                            }
                            opts.unit = Unit::Bytes;
                            unit_seen = true;
                        }
                        Flag::Unknown => return Err(DU_EOPT),
                    }
                    j += 1;
                }
                i += 1;
                continue;
            }
            operands = true;
        }
        if len >= PATH_CAP {
            return Err(DU_ELEN);
        }
        if eq_bytes(a, b"-") {
            return Err(DU_EBYTE);
        }
        let n = Name::new(a)?;
        if let Some(dst) = paths.get_mut(len) {
            *dst = n;
        }
        len += 1;
        i += 1;
    }
    if opts.summarize && opts.all {
        return Err(DU_ESTATE);
    }
    if len == 0 {
        if let Some(dst) = paths.get_mut(0) {
            *dst = Name::new(b".")?;
        }
        len = 1;
    }
    Ok((opts, paths, len))
}

// ---------------------------------------------------------------------------
// 报告
// ---------------------------------------------------------------------------

/// 一次 `du` 的输出。
#[derive(Clone, Debug)]
pub struct Report {
    text: String,
    lines: usize,
    total_blocks: u64,
    nodes: usize,
}

impl Report {
    /// 空白。
    pub fn blank() -> Report {
        Report { text: String::new(), lines: 0, total_blocks: 0, nodes: 0 }
    }

    /// 文本。
    pub fn text(&self) -> &str {
        self.text.as_str()
    }

    /// 行数。
    pub fn lines(&self) -> usize {
        self.lines
    }

    /// 总块数。
    pub fn total_blocks(&self) -> u64 {
        self.total_blocks
    }

    /// 输出节点数。
    pub fn nodes(&self) -> usize {
        self.nodes
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.lines == 0
    }
}

/// 追加一行 `size\tpath\n`。
fn push_line(out: &mut String, size: String, path: &str) {
    out.push_str(size.as_str());
    out.push('\t');
    out.push_str(path);
    out.push('\n');
}

/// 生成报告。
pub fn render(tree: &Tree, opts: &Options, paths: &[Name], len: usize) -> Result<Report, u32> {
    if tree.is_empty() {
        return Err(DU_ENOENT);
    }
    if len == 0 || len > PATH_CAP {
        return Err(DU_ELEN);
    }
    let mut seen = [0u16; PATH_CAP];
    let mut roots = [0u16; PATH_CAP];
    let mut i = 0usize;
    while i < len {
        let n = match paths.get(i) {
            Some(v) => *v,
            None => return Err(DU_ENOENT),
        };
        let idx = tree.find(n.bytes())?;
        let mut k = 0usize;
        while k < i {
            if seen.get(k) == Some(&(idx as u16)) {
                return Err(DU_ESTATE);
            }
            k += 1;
        }
        if let Some(dst) = seen.get_mut(i) {
            *dst = idx as u16;
        }
        if let Some(dst) = roots.get_mut(i) {
            *dst = idx as u16;
        }
        i += 1;
    }
    let mut out = String::new();
    let mut lines = 0usize;
    let mut total = 0u64;
    let mut nodes = 0usize;
    let mut r = 0usize;
    while r < len {
        let root = match roots.get(r) {
            Some(v) => *v as usize,
            None => return Err(DU_ENOENT),
        };
        let root_size = tree.sum_of(root)?;
        total = total.saturating_add(root_size);
        if opts.summarize {
            let p = tree.path_of(root)?;
            push_line(&mut out, opts.render_size(root_size), p.as_str());
            lines += 1;
            nodes += 1;
        } else {
            let mut idx = 0usize;
            while idx < tree.len() {
                if tree.in_subtree(idx, root) {
                    let e = match tree.at(idx) {
                        Some(v) => v,
                        None => return Err(DU_ENOENT),
                    };
                    let show = e.kind == Kind::Dir || opts.all;
                    if show {
                        let size = tree.sum_of(idx)?;
                        let p = tree.path_of(idx)?;
                        push_line(&mut out, opts.render_size(size), p.as_str());
                        lines += 1;
                        nodes += 1;
                    }
                }
                idx += 1;
            }
        }
        r += 1;
    }
    if opts.grand {
        let mut s = String::new();
        push_u64(&mut s, opts.unit.to_bytes(total));
        if opts.human {
            s = human(opts.unit.to_bytes(total));
        }
        out.push_str(s.as_str());
        out.push_str("\ttotal\n");
        lines += 1;
    }
    Ok(Report { text: out, lines, total_blocks: total, nodes })
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
            return Err(DU_ESLOT);
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
            return Err(DU_EBAL);
        }
        const VALS: [u32; 8] = [46, 4, 3, 47104, 2, 4, 25, 1];
        const CODES: [u32; 6] = [DU_ETREE, DU_EDUP, DU_ETYPE, DU_EOPT, DU_ELEN, DU_EBYTE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(DU_EBAL),
            };
            if s.slot as usize != i {
                return Err(DU_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(DU_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(DU_EVALUE);
                        }
                    }
                    None => return Err(DU_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(DU_EVALUE);
                        }
                    }
                    None => return Err(DU_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(DU_EBAL);
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
            return Err(DU_EBAL);
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
            Err(DU_ESEAL)
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
// 样例树
// ---------------------------------------------------------------------------

/// 建样例树：
///
/// ```text
/// .            dir   4
/// ├── a        dir   8
/// │   ├── x    file 12
/// │   └── y    file  5
/// ├── b        dir   6
/// │   └── z    file  9
/// └── c        file  2
/// ```
///
/// 合计 46 块，目录 3 个，文件 4 个，最深 2 层；`a` 子树 25 块。
pub fn sample_tree() -> Result<Tree, u32> {
    let mut t = Tree::new();
    let a = t.push(Kind::Dir, b".", 4, ROOT_PARENT)?;
    let b = t.push(Kind::Dir, b"a", 8, a)?;
    let c = t.push(Kind::File, b"x", 12, b)?;
    let d = t.push(Kind::File, b"y", 5, b)?;
    let e = t.push(Kind::Dir, b"b", 6, a)?;
    let f = t.push(Kind::File, b"z", 9, e)?;
    let g = t.push(Kind::File, b"c", 2, a)?;
    let _ = (c, d, f, g);
    Ok(t)
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

/// 账本组装。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(long_cell(0, OP_SUM, sample_tree().and_then(|t| t.sum_of(0))));

    let _ = l.register(count_cell(
        1,
        OP_COUNT,
        sample_tree().and_then(|t| t.count_kind(0, Kind::File)),
    ));

    let _ = l.register(count_cell(
        2,
        OP_COUNT,
        sample_tree().and_then(|t| t.count_kind(0, Kind::Dir)),
    ));

    let _ = l.register(long_cell(
        3,
        OP_UNIT,
        sample_tree()
            .and_then(|t| t.sum_of(0))
            .map(|b| Unit::K.to_bytes(b)),
    ));

    let _ = l.register(count_cell(4, OP_NODE, sample_tree().and_then(|t| t.max_depth(0))));

    let _ = l.register(count_cell(
        5,
        OP_UNIT,
        Ok(human(KIB).as_str().as_bytes().len()),
    ));

    let _ = l.register(long_cell(
        6,
        OP_SUM,
        sample_tree().and_then(|t| t.find(b"a")).and_then(|i| sample_tree()?.sum_of(i)),
    ));

    let _ = l.register(count_cell(
        7,
        OP_REPORT,
        sample_tree().and_then(|t| {
            let (opts, paths, len) = parse_argv(&[b"-s"])?;
            render(&t, &opts, &paths, len).map(|r| r.lines())
        }),
    ));

    let _ = l.register(long_cell(
        8,
        OP_NODE,
        Tree::new().push(Kind::Dir, b"x", 1, ROOT_PARENT).and_then(|root| {
            let mut t = Tree::new();
            let r = t.push(Kind::Dir, b"x", 1, ROOT_PARENT)?;
            let _ = root;
            let _ = r;
            t.push(Kind::File, b"y", 1, 7).map(|_| 0u64)
        }),
    ));

    let _ = l.register(long_cell(
        9,
        OP_NODE,
        sample_tree().and_then(|mut t| t.push(Kind::File, b"a", 1, 0).map(|_| 0u64)),
    ));

    let _ = l.register(long_cell(
        10,
        OP_NODE,
        sample_tree().and_then(|mut t| t.push(Kind::File, b"w", 1, 2).map(|_| 0u64)),
    ));

    let _ = l.register(long_cell(11, OP_OPT, parse_argv(&[b"-z"]).map(|_| 0u64)));

    let _ = l.register(long_cell(
        12,
        OP_OPT,
        parse_argv(&[b"-s", b"p1", b"p2", b"p3", b"p4", b"p5"]).map(|_| 0u64),
    ));

    let _ = l.register(long_cell(13, OP_OPT, parse_argv(&[b"\x01"]).map(|_| 0u64)));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 装载面。
fn build_ok() -> bool {
    let a = match sample_tree() {
        Ok(t) => t.len() == 7,
        Err(_) => false,
    };
    let b = match sample_tree() {
        Ok(t) => t.depth(0) == Ok(0) && t.depth(1) == Ok(1) && t.depth(2) == Ok(2),
        Err(_) => false,
    };
    let c = match sample_tree() {
        Ok(t) => t.at(2).map(|e| e.kind == Kind::File && e.blocks == 12).unwrap_or(false),
        Err(_) => false,
    };
    let d = Tree::new().push(Kind::File, b"x", 1, ROOT_PARENT).is_ok();
    let e = Tree::new().push(Kind::File, b"x", 1, 0).err() == Some(DU_ETREE);
    a && b && c && d && e
}

/// 父子与类型面。
fn parent_ok() -> bool {
    let a = sample_tree()
        .and_then(|mut t| t.push(Kind::File, b"q", 1, 9).map(|_| 0u64))
        .err()
        == Some(DU_ETREE);
    let b = sample_tree().and_then(|mut t| t.push(Kind::File, b"x", 1, 2)).err() == Some(DU_ETYPE);
    let c = sample_tree().and_then(|mut t| t.push(Kind::Dir, b"a", 1, 0)).err() == Some(DU_EDUP);
    let d = sample_tree().and_then(|mut t| t.push(Kind::File, b"", 1, 0)).err() == Some(DU_EBYTE);
    let e = sample_tree()
        .and_then(|mut t| t.push(Kind::File, &[b'x'; 33], 1, 0))
        .err()
        == Some(DU_ELEN);
    let f = sample_tree().and_then(|mut t| t.push(Kind::File, b"ok", 1, 0)).is_ok();
    a && b && c && d && e && f
}

/// 路径面。
fn path_ok() -> bool {
    let t = match sample_tree() {
        Ok(v) => v,
        Err(_) => Tree::new(),
    };
    let a = t.path_of(0).map(|p| p.as_str() == ".") == Ok(true);
    let b = t.path_of(1).map(|p| p.as_str() == "a") == Ok(true);
    let c = t.path_of(2).map(|p| p.as_str() == "a/x") == Ok(true);
    let d = t.path_of(4).map(|p| p.as_str() == "b") == Ok(true);
    let e = t.path_of(5).map(|p| p.as_str() == "b/z") == Ok(true);
    let f = t.find(b"a/x") == Ok(2);
    let g = t.find(b"nope").err() == Some(DU_ENOENT);
    a && b && c && d && e && f && g
}

/// 求和在面。
fn sum_ok() -> bool {
    let t = match sample_tree() {
        Ok(v) => v,
        Err(_) => Tree::new(),
    };
    let a = t.sum_of(0) == Ok(46);
    let b = t.sum_of(1) == Ok(25);
    let c = t.sum_of(4) == Ok(15);
    let d = t.sum_of(2) == Ok(12);
    let e = t.count_kind(0, Kind::Dir) == Ok(3);
    let f = t.count_kind(0, Kind::File) == Ok(4);
    let g = t.max_depth(0) == Ok(2);
    let h = t.max_depth(1) == Ok(1);
    let i = t.in_subtree(2, 0) && !t.in_subtree(5, 1);
    a && b && c && d && e && f && g && h && i
}

/// 单位面。
fn unit_ok() -> bool {
    let a = Unit::K.to_bytes(46) == 47_104;
    let b = Unit::HalfK.to_bytes(2) == 1_024;
    let c = Unit::M.to_bytes(1) == 1;
    let d = Unit::M.to_bytes(1025) == 2;
    let e = Unit::M.to_bytes(1024) == 1;
    let f = Unit::Bytes.to_bytes(3) == 3_072;
    let g = human(0).as_str() == "0B";
    let h = human(1023).as_str() == "1023B";
    let i = human(1024).as_str() == "1.0K";
    let j = human(1536).as_str() == "1.5K";
    let k = human(10240).as_str() == "10K";
    let l = human(1_048_576).as_str() == "1.0M";
    let m = human(1_610_612_736).as_str() == "1.5G";
    a && b && c && d && e && f && g && h && i && j && k && l && m
}

/// 选项面。
fn option_ok() -> bool {
    let a = match parse_argv(&[b"-s"]) {
        Ok((o, _, len)) => o.summarize && len == 1,
        Err(_) => false,
    };
    let b = match parse_argv(&[b"-ah"]) {
        Ok((o, _, _)) => o.all && o.human,
        Err(_) => false,
    };
    let c = match parse_argv(&[b"-c", b"a"]) {
        Ok((o, p, len)) => o.grand && len == 1 && p.get(0).map(|n| n.named(b"a")).unwrap_or(false),
        Err(_) => false,
    };
    let d = match parse_argv(&[b"-k"]) {
        Ok((o, _, _)) => o.unit == Unit::K,
        Err(_) => false,
    };
    let e = match parse_argv(&[b"-m"]) {
        Ok((o, _, _)) => o.unit == Unit::M,
        Err(_) => false,
    };
    let f = match parse_argv(&[b"-b"]) {
        Ok((o, _, _)) => o.unit == Unit::Bytes,
        Err(_) => false,
    };
    let g = match parse_argv(&[b"--", b"-s"]) {
        Ok((o, p, len)) => !o.summarize && len == 1 && p.get(0).map(|n| n.named(b"-s")).unwrap_or(false),
        Err(_) => false,
    };
    let h = parse_argv(&[b"-s", b"-a"]).err() == Some(DU_ESTATE);
    let i = parse_argv(&[b"-k", b"-m"]).err() == Some(DU_ESTATE);
    a && b && c && d && e && f && g && h && i
}

/// 选项错误面。
fn option_err_ok() -> bool {
    let a = parse_argv(&[b"-z"]).err() == Some(DU_EOPT);
    let b = parse_argv(&[b"--all"]).err() == Some(DU_EOPT);
    let c = parse_argv(&[b"p1", b"p2", b"p3", b"p4", b"p5"]).err() == Some(DU_ELEN);
    let d = parse_argv(&[b"\x02"]).err() == Some(DU_EBYTE);
    let e = parse_argv(&[b"-"]).err() == Some(DU_EBYTE);
    a && b && c && d && e
}

/// 报告面。
fn report_ok() -> bool {
    let t = match sample_tree() {
        Ok(v) => v,
        Err(_) => Tree::new(),
    };
    let (o1, p1, l1) = match parse_argv(&[b"-s"]) {
        Ok(v) => v,
        Err(_) => (Options::new(), [Name::blank(); PATH_CAP], 0),
    };
    let r1 = match render(&t, &o1, &p1, l1) {
        Ok(v) => v,
        Err(_) => Report::blank(),
    };
    let a = r1.lines() == 1 && r1.total_blocks() == 46;
    let b = r1.text().as_bytes() == b"47104\t.\n";
    let (o2, p2, l2) = match parse_argv(&[b"-a"]) {
        Ok(v) => v,
        Err(_) => (Options::new(), [Name::blank(); PATH_CAP], 0),
    };
    let r2 = match render(&t, &o2, &p2, l2) {
        Ok(v) => v,
        Err(_) => Report::blank(),
    };
    let c = r2.lines() == 7;
    let d = r2.text().contains("12288\ta/x\n");
    let e = r2.text().contains("5120\ta/y\n");
    let f = r2.text().contains("2048\tc\n");
    let (o3, p3, l3) = match parse_argv(&[b"-a", b"a"]) {
        Ok(v) => v,
        Err(_) => (Options::new(), [Name::blank(); PATH_CAP], 0),
    };
    let r3 = match render(&t, &o3, &p3, l3) {
        Ok(v) => v,
        Err(_) => Report::blank(),
    };
    let g = r3.lines() == 3 && r3.total_blocks() == 25;
    a && b && c && d && e && f && g
}

/// 人读与合计面。
fn human_report_ok() -> bool {
    let t = match sample_tree() {
        Ok(v) => v,
        Err(_) => Tree::new(),
    };
    let (o, p, l) = match parse_argv(&[b"-sh"]) {
        Ok(v) => v,
        Err(_) => (Options::new(), [Name::blank(); PATH_CAP], 0),
    };
    let r = match render(&t, &o, &p, l) {
        Ok(v) => v,
        Err(_) => Report::blank(),
    };
    let a = r.text() == "46K\t.\n";
    let (o2, p2, l2) = match parse_argv(&[b"-c"]) {
        Ok(v) => v,
        Err(_) => (Options::new(), [Name::blank(); PATH_CAP], 0),
    };
    let r2 = match render(&t, &o2, &p2, l2) {
        Ok(v) => v,
        Err(_) => Report::blank(),
    };
    let b = r2.lines() == 4;
    let c = r2.text().ends_with("47104\ttotal\n");
    let d = r2.text().starts_with("47104\t.\n");
    let e = r2.text().contains("25600\ta\n");
    a && b && c && d && e
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
    if l2.register(probe) != Err(DU_ESLOT) {
        return false;
    }
    l2.rejects() == 1
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = build_ok() && parent_ok();
    out[1] = path_ok() && sum_ok();
    out[2] = ledger_ok();
    out[3] = unit_ok() && option_ok() && option_err_ok() && report_ok() && human_report_ok();
    out
}

/// 判据集合。
pub fn run_unx11544_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11544.du");

    s.add(
        "DU01-装载与深度",
        {
            build_ok()
        },
        "static",
    );

    s.add(
        "DU02-父子与重名",
        {
            parent_ok()
        },
        "static",
    );

    s.add(
        "DU03-路径拼接",
        {
            path_ok()
        },
        "static",
    );

    s.add(
        "DU04-子树求和",
        {
            sum_ok()
        },
        "static",
    );

    s.add(
        "DU05-单位换算",
        {
            unit_ok()
        },
        "static",
    );

    s.add(
        "DU06-选项组合",
        {
            option_ok()
        },
        "static",
    );

    s.add(
        "DU07-选项错误码",
        {
            option_err_ok()
        },
        "static",
    );

    s.add(
        "DU08-报告矩形",
        {
            report_ok()
        },
        "static",
    );

    s.add(
        "DU09-人读与合计",
        {
            human_report_ok()
        },
        "static",
    );

    s.add(
        "DU10-容量边界",
        {
            let a = Tree::new().push(Kind::File, &[b'q'; 33], 1, ROOT_PARENT).err() == Some(DU_ELEN);
            let b = Name::new(b"").err() == Some(DU_EBYTE);
            let c = parse_argv(&[b"p1", b"p2", b"p3", b"p4", b"p5"]).err() == Some(DU_ELEN);
            let d = sample_tree().map(|t| t.len()) == Ok(7);
            a && b && c && d
        },
        "static",
    );

    s.add(
        "DU11-账本对账",
        {
            build_ledger().audit() == Ok(())
        },
        "static",
    );

    s.add(
        "DU12-空树拒绝",
        {
            let t = Tree::new();
            render(&t, &Options::new(), &[Name::blank(); PATH_CAP], 1).err() == Some(DU_ENOENT)
                && t.root().is_none()
                && t.sum_of(0).err() == Some(DU_ENOENT)
        },
        "static",
    );

    s.add(
        "DU13-冻结重放四注入",
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
    fn tree_shape() {
        let t = sample_tree().expect("t");
        assert_eq!(t.len(), 7);
        assert_eq!(t.root(), Some(0));
        assert_eq!(t.depth(0), Ok(0));
        assert_eq!(t.depth(2), Ok(2));
        assert_eq!(t.at(3).map(|e| e.blocks), Some(5));
        assert_eq!(t.at(2).map(|e| e.kind), Some(Kind::File));
    }

    #[test]
    fn tree_gates() {
        assert_eq!(Tree::new().push(Kind::File, b"x", 1, 0).err(), Some(DU_ETREE));
        assert_eq!(
            sample_tree().and_then(|mut t| t.push(Kind::File, b"x", 1, 2)).err(),
            Some(DU_ETYPE)
        );
        assert_eq!(
            sample_tree().and_then(|mut t| t.push(Kind::Dir, b"a", 1, 0)).err(),
            Some(DU_EDUP)
        );
        assert_eq!(
            sample_tree().and_then(|mut t| t.push(Kind::File, b"", 1, 0)).err(),
            Some(DU_EBYTE)
        );
        assert_eq!(
            sample_tree().and_then(|mut t| t.push(Kind::File, &[b'x'; 33], 1, 0)).err(),
            Some(DU_ELEN)
        );
    }

    #[test]
    fn paths() {
        let t = sample_tree().expect("t");
        assert_eq!(t.path_of(0).map(|p| p.as_str().to_string()), Ok(String::from(".")));
        assert_eq!(t.path_of(1).map(|p| p.as_str().to_string()), Ok(String::from("a")));
        assert_eq!(t.path_of(2).map(|p| p.as_str().to_string()), Ok(String::from("a/x")));
        assert_eq!(t.path_of(5).map(|p| p.as_str().to_string()), Ok(String::from("b/z")));
        assert_eq!(t.find(b"b/z"), Ok(5));
        assert_eq!(t.find(b"zzz").err(), Some(DU_ENOENT));
    }

    #[test]
    fn sums() {
        let t = sample_tree().expect("t");
        assert_eq!(t.sum_of(0), Ok(46));
        assert_eq!(t.sum_of(1), Ok(25));
        assert_eq!(t.sum_of(2), Ok(12));
        assert_eq!(t.sum_of(4), Ok(15));
        assert_eq!(t.sum_of(5), Ok(9));
        assert_eq!(t.count_kind(0, Kind::Dir), Ok(3));
        assert_eq!(t.count_kind(0, Kind::File), Ok(4));
        assert_eq!(t.max_depth(0), Ok(2));
    }

    #[test]
    fn units() {
        assert_eq!(Unit::K.to_bytes(46), 47_104);
        assert_eq!(Unit::HalfK.to_bytes(3), 1_536);
        assert_eq!(Unit::M.to_bytes(1024), 1);
        assert_eq!(Unit::M.to_bytes(1025), 2);
        assert_eq!(Unit::Bytes.text(), "1B");
    }

    #[test]
    fn human_sizes() {
        assert_eq!(human(0).as_str(), "0B");
        assert_eq!(human(512).as_str(), "512B");
        assert_eq!(human(1024).as_str(), "1.0K");
        assert_eq!(human(1536).as_str(), "1.5K");
        assert_eq!(human(10240).as_str(), "10K");
        assert_eq!(human(10_240 + 1).as_str(), "11K");
        assert_eq!(human(1_048_576).as_str(), "1.0M");
        assert_eq!(human(1_610_612_736).as_str(), "1.5G");
        assert_eq!(human(47_104).as_str(), "46K");
    }

    #[test]
    fn option_parsing() {
        let (o, p, l) = parse_argv(&[b"-ahc", b"a"]).expect("o");
        assert!(!o.summarize && o.all && o.human && o.grand);
        assert_eq!(l, 1);
        assert!(p.get(0).map(|n| n.named(b"a")).unwrap_or(false));
        let (o2, p2, l2) = parse_argv(&[]).expect("o2");
        assert_eq!(l2, 1);
        assert!(p2.get(0).map(|n| n.named(b".")).unwrap_or(false));
        assert_eq!(o2.unit, Unit::K);
    }

    #[test]
    fn option_errors() {
        assert_eq!(parse_argv(&[b"-s", b"-a"]).err(), Some(DU_ESTATE));
        assert_eq!(parse_argv(&[b"-k", b"-m"]).err(), Some(DU_ESTATE));
        assert_eq!(parse_argv(&[b"-q"]).err(), Some(DU_EOPT));
        assert_eq!(parse_argv(&[b"--human"]).err(), Some(DU_EOPT));
        assert_eq!(parse_argv(&[b"-"]).err(), Some(DU_EBYTE));
        assert_eq!(parse_argv(&[b"\x03"]).err(), Some(DU_EBYTE));
        assert_eq!(
            parse_argv(&[b"p1", b"p2", b"p3", b"p4", b"p5"]).err(),
            Some(DU_ELEN)
        );
    }

    #[test]
    fn summarize_render() {
        let t = sample_tree().expect("t");
        let (o, p, l) = parse_argv(&[b"-s"]).expect("o");
        let r = render(&t, &o, &p, l).expect("r");
        assert_eq!(r.lines(), 1);
        assert_eq!(r.total_blocks(), 46);
        assert_eq!(r.text(), "47104\t.\n");
    }

    #[test]
    fn all_render() {
        let t = sample_tree().expect("t");
        let (o, p, l) = parse_argv(&[b"-a"]).expect("o");
        let r = render(&t, &o, &p, l).expect("r");
        assert_eq!(r.lines(), 7);
        assert!(r.text().contains("47104\t.\n"));
        assert!(r.text().contains("25600\ta\n"));
        assert!(r.text().contains("12288\ta/x\n"));
        assert!(r.text().contains("5120\ta/y\n"));
        assert!(r.text().contains("15360\tb\n"));
        assert!(r.text().contains("9216\tb/z\n"));
        assert!(r.text().contains("2048\tc\n"));
    }

    #[test]
    fn subtree_render() {
        let t = sample_tree().expect("t");
        let (o, p, l) = parse_argv(&[b"-a", b"a"]).expect("o");
        let r = render(&t, &o, &p, l).expect("r");
        assert_eq!(r.lines(), 3);
        assert_eq!(r.total_blocks(), 25);
        assert!(r.text().starts_with("25600\ta\n"));
        assert!(r.text().contains("12288\ta/x\n"));
    }

    #[test]
    fn grand_total() {
        let t = sample_tree().expect("t");
        let (o, p, l) = parse_argv(&[b"-c"]).expect("o");
        let r = render(&t, &o, &p, l).expect("r");
        assert_eq!(r.lines(), 4);
        assert!(r.text().ends_with("47104\ttotal\n"));
        let (o2, p2, l2) = parse_argv(&[b"-c", b"-h"]).expect("o2");
        let r2 = render(&t, &o2, &p2, l2).expect("r2");
        assert!(r2.text().contains("total\n"));
    }

    #[test]
    fn human_render() {
        let t = sample_tree().expect("t");
        let (o, p, l) = parse_argv(&[b"-sh"]).expect("o");
        let r = render(&t, &o, &p, l).expect("r");
        assert_eq!(r.text(), "46K\t.\n");
    }

    #[test]
    fn path_errors() {
        let t = sample_tree().expect("t");
        let (o, _, _) = parse_argv(&[]).expect("o");
        let ghost = [Name::new(b"ghost").expect("g"), Name::blank(), Name::blank(), Name::blank()];
        assert_eq!(render(&t, &o, &ghost, 1).err(), Some(DU_ENOENT));
        let twice = [Name::new(b"a").expect("a"), Name::new(b"a").expect("a"), Name::blank(), Name::blank()];
        assert_eq!(render(&t, &o, &twice, 2).err(), Some(DU_ESTATE));
        assert_eq!(render(&t, &o, &ghost, 0).err(), Some(DU_ELEN));
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.sample(0).map(|s| s.value), Some(46));
        assert_eq!(l.sample(3).map(|s| s.value), Some(47_104));
        assert_eq!(l.sample(9).map(|s| s.code), Some(DU_EDUP));
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = l.freeze().expect("freeze");
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(DU_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(DU_ESLOT));
        assert_eq!(l.rejects(), 2);
    }

    #[test]
    fn injections() {
        let four = inject_four();
        assert!(four[0] && four[1] && four[2] && four[3]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11544_checks();
        assert!(s.all_passed());
    }
}
