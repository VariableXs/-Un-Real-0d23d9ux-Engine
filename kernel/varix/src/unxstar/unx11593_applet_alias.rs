//! UNX-F11593 applet 别名与内建表判据
//!
//! busybox 的 applet 面是「一表两名」的结构：**规范名**（canonical，真正
//! 带 `main` 的实现）与**别名**（alias，安装期做成同一 inode 的硬/软链接）。
//! 用户敲的 `[`、`egrep`、`sh`、`zcat` 全部落在别名侧，最终都必须收敛到
//! 一个规范名，否则分派器只能回 `applet not found`。判据因此不是「表里有
//! 没有这一行」，而是四件事同时成立：
//!
//! ```text
//! 有序   别名表按名字字节序严格递增（分派走二分，乱序等于漏项）
//! 可达   每个别名的目标在 ≤ HOP_CAP 跳内收敛到规范名（zcat→gunzip→gzip）
//! 不遮蔽 别名名不得与规范名重名（ls→cat 这种会把真 ls 顶掉）
//! 自洽   自指别名、重复别名、悬空目标、超容量一律拒，且拒绝要计数
//! ```
//!
//! 本模块把这些规则做成定容表（12 槽 × 两个 16 字节名），**零分配**：
//! 插入即有序（先二分定插入点再右移），查询即二分，链深用固定上界
//! `HOP_CAP = 2` 截断——链过长不会被误判成「可解析」，而是明确落
//! `AL_ECAP`（结构越界：表满或链深越界共用一码，二者都是定容结构的
//! 溢出，语义上同族）。
//!
//! 分派面另立一个四态模型：`Canonical(i)`（规范名直接命中）、
//! `Alias{canon,hops}`（别名收敛，带跳数）、`Dispatcher`（`busybox` 自身，
//! 不是 applet，是内建分派入口）、`Unknown`。四态的编码互不重叠，便于
//! 上层把「敲了 busybox」与「敲了不存在的名字」区分开——前者应打印用法，
//! 后者应回 127。
//!
//! 闸门次序：字节 → 长度 → 空名 → 斜杠 → 规范名 → 别名二分 → 链深 → 分派。
//!
//! 账本 14 格（8 成 6 败）在 `audit` 逐格对账：成功格对 `VALS`，失败格对
//! `CODES`，另验格数、成败计数、槽位次序与「成功格错误码必须为 0」。冻结
//! 后重放须同指纹；本模块无 std、无 unsafe、无 panic、无 unwrap。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（本卡专属）。
pub const SEG_BASE: u32 = 0xfa60;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 单个名字的字节容量（applet 名最长 `start-stop-daemon` 已足够）。
pub const NAME_CAP: usize = 16;

/// 别名表槽数（**正好**装下标准表 11 条，第 12 条是容量边界）。
pub const ALIAS_CAP: usize = 12;

/// 别名链允许的最大跳数。
pub const HOP_CAP: u8 = 2;

/// 分派编码基数：规范名从 0x10 起。
pub const CODE_CANON: u32 = 0x10;

/// 分派编码基数：别名从 0x20 起。
pub const CODE_ALIAS: u32 = 0x20;

/// 分派编码：`busybox` 自身。
pub const CODE_DISPATCH: u32 = 0xff;

/// 分派编码：未知。
pub const CODE_UNKNOWN: u32 = 0xee;

// ---------------------------------------------------------------------------
// 错误码：8 个域错误 + 5 个账本错误
// ---------------------------------------------------------------------------

/// 名含非打印字节（0x21..=0x7e 之外）。
pub const AL_EBYTE: u32 = SEG_BASE + 0x01;
/// 名超长（> NAME_CAP）。
pub const AL_ELEN: u32 = SEG_BASE + 0x02;
/// 空名。
pub const AL_EEMPTY: u32 = SEG_BASE + 0x03;
/// 名含斜杠（别名只能是裸名，不能是路径）。
pub const AL_ESLASH: u32 = SEG_BASE + 0x04;
/// 别名重复（含自指别名）。
pub const AL_EDUP: u32 = SEG_BASE + 0x05;
/// 别名遮蔽规范名（重名即拒，不论目标是否相同）。
pub const AL_ESHADOW: u32 = SEG_BASE + 0x06;
/// 别名目标不存在（既非规范名也非表内别名）。
pub const AL_ETARGET: u32 = SEG_BASE + 0x07;
/// 结构越界：表已满，或别名链深超过 HOP_CAP。
pub const AL_ECAP: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const AL_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法（乱序、重复登记、冻结后登记）。
pub const AL_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const AL_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const AL_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const AL_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 查别名。
pub const OP_LOOKUP: u8 = 0x01;
/// 插别名。
pub const OP_ADD: u8 = 0x02;
/// 表形状。
pub const OP_SHAPE: u8 = 0x03;
/// 按目标计数。
pub const OP_COUNT: u8 = 0x04;
/// 分派。
pub const OP_DISPATCH: u8 = 0x05;
/// 链深。
pub const OP_CHAIN: u8 = 0x06;

// ---------------------------------------------------------------------------
// 哈希：FNV-1a，每字节 8 次乘法
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

/// 折入一个字（小端逐字节）。
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

/// 折入一段字节。
fn fold_bytes(mut h: u64, s: &[u8]) -> u64 {
    let mut i = 0usize;
    while i < s.len() {
        if let Some(b) = s.get(i) {
            h = fold_byte(h, *b);
        }
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 字节助手
// ---------------------------------------------------------------------------

/// 字节序比较：-1 / 0 / 1。
fn cmp_bytes(a: &[u8], b: &[u8]) -> i32 {
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

/// 位置下标（0 起）拼进 String，供诊断文本用。
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
        at = at.saturating_sub(1);
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
// 规范名表（12 条，字节序）
// ---------------------------------------------------------------------------

/// 规范 applet 名（严格字节序）。
///
/// 这 12 条是本卡判据的「落地集」：别名只能收敛到其中之一；表外名字一律
/// `Unknown`。顺序即编号，编号进分派编码，改动等于改协议，故写死。
pub const CANON: [&str; 12] = [
    "ash", "awk", "cat", "grep", "gzip", "init", "ls", "ps", "sed", "tar", "test", "vi",
];

/// 规范名二分查找。
pub fn canon_index(name: &[u8]) -> Option<u8> {
    let mut lo = 0usize;
    let mut hi = CANON.len();
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        match CANON.get(mid) {
            Some(t) => match cmp_bytes(t.as_bytes(), name) {
                c if c < 0 => lo = mid + 1,
                c if c > 0 => hi = mid,
                _ => return Some(mid as u8),
            },
            None => return None,
        }
    }
    None
}

/// 规范名文本（越界回空串语义的 `""`，但不分配）。
pub fn canon_name(i: u8) -> &'static str {
    match CANON.get(i as usize) {
        Some(v) => v,
        None => "",
    }
}

/// 规范名表是否严格递增（自检用，同时验证常量表没被写乱）。
pub fn canon_sorted() -> bool {
    let mut i = 1usize;
    while i < CANON.len() {
        match (CANON.get(i - 1), CANON.get(i)) {
            (Some(a), Some(b)) => {
                if cmp_bytes(a.as_bytes(), b.as_bytes()) >= 0 {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 名字
// ---------------------------------------------------------------------------

/// 定容裸名（applet 名 / 别名）。
///
/// 不变量：`len <= NAME_CAP`，字节全在 `0x21..=0x7e`，不含 `/`。
/// `blank()` 是「零名」，只作为表槽的初值，绝不参与成功路径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Name {
    buf: [u8; NAME_CAP],
    len: u8,
}

impl Name {
    /// 零名。
    pub const fn blank() -> Name {
        Name { buf: [0u8; NAME_CAP], len: 0 }
    }

    /// 校验并构造：空 → EBYTE/LEN 族错误；坏字节 → EBYTE；斜杠 → ESLASH。
    pub fn new(raw: &[u8]) -> Result<Name, u32> {
        if raw.is_empty() {
            return Err(AL_EEMPTY);
        }
        if raw.len() > NAME_CAP {
            return Err(AL_ELEN);
        }
        let mut i = 0usize;
        while i < raw.len() {
            match raw.get(i) {
                Some(b) => {
                    if *b < 0x21 || *b > 0x7e {
                        return Err(AL_EBYTE);
                    }
                    if *b == b'/' {
                        return Err(AL_ESLASH);
                    }
                }
                None => return Err(AL_EBYTE),
            }
            i += 1;
        }
        let mut buf = [0u8; NAME_CAP];
        let mut k = 0usize;
        while k < raw.len() {
            match (buf.get_mut(k), raw.get(k)) {
                (Some(dst), Some(src)) => *dst = *src,
                _ => return Err(AL_EBYTE),
            }
            k += 1;
        }
        Ok(Name { buf, len: raw.len() as u8 })
    }

    /// 视图（长度已夹紧，绝不越界切片）。
    pub fn as_bytes(&self) -> &[u8] {
        let n = (self.len as usize).min(NAME_CAP);
        match self.buf.get(..n) {
            Some(s) => s,
            None => &[],
        }
    }

    /// 字节长度。
    pub fn len(&self) -> usize {
        self.as_bytes().len()
    }

    /// 是否零名。
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 文本视图（诊断用）。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len() {
            if let Some(b) = self.as_bytes().get(i) {
                s.push(*b as char);
            }
            i += 1;
        }
        s
    }

    /// 同名判定。
    pub fn eq_name(&self, other: &Name) -> bool {
        eq_bytes(self.as_bytes(), other.as_bytes())
    }
}

/// 宽松构造：失败时回零名（只用于「先证明成功再读」的场景）。
fn nm(raw: &[u8]) -> Name {
    match Name::new(raw) {
        Ok(v) => v,
        Err(_) => Name::blank(),
    }
}

// ---------------------------------------------------------------------------
// 别名
// ---------------------------------------------------------------------------

/// 一条别名：`name` 是用户敲的名字，`target` 是它指向的名字。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Alias {
    /// 别名本体。
    pub name: Name,
    /// 目标（规范名或另一条别名）。
    pub target: Name,
}

impl Alias {
    /// 空槽。
    pub const fn blank() -> Alias {
        Alias { name: Name::blank(), target: Name::blank() }
    }

    /// 构造：两名都过名闸；自指（name == target）直接判重复。
    pub fn new(name: &[u8], target: &[u8]) -> Result<Alias, u32> {
        let n = Name::new(name)?;
        let t = Name::new(target)?;
        if n.eq_name(&t) {
            return Err(AL_EDUP);
        }
        Ok(Alias { name: n, target: t })
    }

    /// 文本形态 `name->target`。
    pub fn text(&self) -> String {
        let mut s = self.name.text();
        s.push_str("->");
        s.push_str(self.target.text().as_str());
        s
    }
}

// ---------------------------------------------------------------------------
// 解析结果
// ---------------------------------------------------------------------------

/// 一次解析的落点：规范名编号 + 跳数（0 表示本身就是规范名）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    /// 规范名编号。
    pub canon: u8,
    /// 跳数。
    pub hops: u8,
}

/// 分派四态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 规范名直命中。
    Canonical(u8),
    /// 别名收敛。
    Alias {
        /// 规范名编号。
        canon: u8,
        /// 跳数。
        hops: u8,
    },
    /// `busybox` 自身：内建分派入口，不是 applet。
    Dispatcher,
    /// 未知。
    Unknown,
}

impl Kind {
    /// 编码（四态互不重叠）。
    pub fn code(&self) -> u32 {
        match self {
            Kind::Canonical(i) => CODE_CANON.saturating_add(*i as u32),
            Kind::Alias { canon, hops } => CODE_ALIAS
                .saturating_add((*canon as u32).saturating_mul(4))
                .saturating_add(*hops as u32),
            Kind::Dispatcher => CODE_DISPATCH,
            Kind::Unknown => CODE_UNKNOWN,
        }
    }

    /// 标签。
    pub fn tag(&self) -> &'static str {
        match self {
            Kind::Canonical(_) => "canon",
            Kind::Alias { .. } => "alias",
            Kind::Dispatcher => "dispatcher",
            Kind::Unknown => "unknown",
        }
    }

    /// 是否内建分派入口。
    pub fn is_builtin(&self) -> bool {
        matches!(self, Kind::Dispatcher)
    }

    /// 是否可落地（规范名或别名）。
    pub fn is_known(&self) -> bool {
        matches!(self, Kind::Canonical(_) | Kind::Alias { .. })
    }

    /// 是否需要真实实现体（别名收敛后仍要落到实现）。
    pub fn needs_impl(&self) -> bool {
        self.is_known()
    }
}

// ---------------------------------------------------------------------------
// 表形状
// ---------------------------------------------------------------------------

/// 表形状摘要。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// 别名条数。
    pub aliases: usize,
    /// 规范名条数。
    pub canonicals: usize,
    /// 最大跳数。
    pub max_hops: u8,
    /// 安装期将创建的链接数（别名 + 规范名各一条）。
    pub links: usize,
}

// ---------------------------------------------------------------------------
// 别名表（定容、恒有序）
// ---------------------------------------------------------------------------

/// 定容别名表。
///
/// 插入即有序：先二分求出插入点，再整体右移一格。查询同样二分。
/// `rejects` 记录被拒次数（不静默），并折进指纹。
#[derive(Clone, Copy, Debug)]
pub struct AliasTable {
    rows: [Alias; ALIAS_CAP],
    len: usize,
    rejects: u32,
}

/// 在有序行数组上求下界位置。
fn row_pos(rows: &[Alias; ALIAS_CAP], len: usize, name: &Name) -> usize {
    let mut lo = 0usize;
    let mut hi = len.min(ALIAS_CAP);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let c = match rows.get(mid) {
            Some(a) => cmp_bytes(a.name.as_bytes(), name.as_bytes()),
            None => 0,
        };
        if c < 0 {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

/// 在有序行数组上精确查找。
fn row_find(rows: &[Alias; ALIAS_CAP], len: usize, name: &Name) -> Option<Alias> {
    let p = row_pos(rows, len, name);
    if p >= len {
        return None;
    }
    match rows.get(p) {
        Some(a) => {
            if a.name.eq_name(name) {
                Some(*a)
            } else {
                None
            }
        }
        None => None,
    }
}

/// 解析（有界跳数）：规范名 0 跳，别名按 target 逐跳跟进，≤ HOP_CAP 跳。
fn resolve_in(rows: &[Alias; ALIAS_CAP], len: usize, name: &Name) -> Option<Hit> {
    if let Some(c) = canon_index(name.as_bytes()) {
        return Some(Hit { canon: c, hops: 0 });
    }
    let mut cur = *name;
    let mut hops = 0u8;
    while hops < HOP_CAP {
        let a = match row_find(rows, len, &cur) {
            Some(v) => v,
            None => return None,
        };
        hops = hops.saturating_add(1);
        if let Some(c) = canon_index(a.target.as_bytes()) {
            return Some(Hit { canon: c, hops });
        }
        cur = a.target;
    }
    None
}

impl AliasTable {
    /// 空表。
    pub const fn new() -> AliasTable {
        AliasTable { rows: [Alias::blank(); ALIAS_CAP], len: 0, rejects: 0 }
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 被拒次数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 取行。
    pub fn at(&self, i: usize) -> Option<Alias> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 插一条别名，返回插入位置。
    ///
    /// 闸门次序：容量 → 遮蔽 → 重复 → 目标可达。四条全部先于「真正写入」，
    /// 失败时表一字不动，只把 `rejects` 加一。
    pub fn add(&mut self, a: Alias) -> Result<usize, u32> {
        if self.len >= ALIAS_CAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(AL_ECAP);
        }
        if canon_index(a.name.as_bytes()).is_some() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(AL_ESHADOW);
        }
        if row_find(&self.rows, self.len, &a.name).is_some() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(AL_EDUP);
        }
        if resolve_in(&self.rows, self.len, &a.target).is_none() {
            self.rejects = self.rejects.saturating_add(1);
            if canon_index(a.target.as_bytes()).is_some()
                || row_find(&self.rows, self.len, &a.target).is_some()
            {
                return Err(AL_ECAP);
            }
            return Err(AL_ETARGET);
        }
        let p = row_pos(&self.rows, self.len, &a.name);
        let mut i = self.len;
        while i > p {
            match (self.rows.get(i - 1).copied(), self.rows.get_mut(i)) {
                (Some(src), Some(dst)) => *dst = src,
                _ => break,
            }
            i -= 1;
        }
        if let Some(dst) = self.rows.get_mut(p) {
            *dst = a;
        }
        self.len = self.len.saturating_add(1);
        Ok(p)
    }

    /// 精确查一行。
    pub fn find(&self, name: &[u8]) -> Option<Alias> {
        row_find(&self.rows, self.len, &nm(name))
    }

    /// 解析名字。
    pub fn resolve(&self, name: &Name) -> Option<Hit> {
        resolve_in(&self.rows, self.len, name)
    }

    /// 解析裸字节名。
    pub fn resolve_bytes(&self, name: &[u8]) -> Option<Hit> {
        resolve_in(&self.rows, self.len, &nm(name))
    }

    /// 收敛到某规范名的别名条数。
    pub fn count_to(&self, canon: u8) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(a) = self.rows.get(i) {
                if let Some(h) = resolve_in(&self.rows, self.len, &a.name) {
                    if h.canon == canon {
                        n += 1;
                    }
                }
            }
            i += 1;
        }
        n
    }

    /// 最长别名的字节长度。
    pub fn longest_alias(&self) -> usize {
        let mut m = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(a) = self.rows.get(i) {
                let l = a.name.len();
                if l > m {
                    m = l;
                }
            }
            i += 1;
        }
        m
    }

    /// 行是否严格按名字递增（结构不变量自检）。
    pub fn is_sorted(&self) -> bool {
        let mut i = 1usize;
        while i < self.len {
            match (self.rows.get(i - 1), self.rows.get(i)) {
                (Some(p), Some(c)) => {
                    if cmp_bytes(p.name.as_bytes(), c.name.as_bytes()) >= 0 {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 形状闸：有序、不遮蔽、全可达、链深不越界。
    pub fn shape(&self) -> Result<Shape, u32> {
        if self.len > ALIAS_CAP {
            return Err(AL_ECAP);
        }
        if !self.is_sorted() {
            return Err(AL_EDUP);
        }
        let mut max_hops = 0u8;
        let mut i = 0usize;
        while i < self.len {
            let a = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(AL_ECAP),
            };
            if canon_index(a.name.as_bytes()).is_some() {
                return Err(AL_ESHADOW);
            }
            match resolve_in(&self.rows, self.len, &a.name) {
                Some(h) => {
                    if h.hops > max_hops {
                        max_hops = h.hops;
                    }
                }
                None => return Err(AL_ECAP),
            }
            i += 1;
        }
        Ok(Shape {
            aliases: self.len,
            canonicals: CANON.len(),
            max_hops,
            links: self.len.saturating_add(CANON.len()),
        })
    }

    /// 指纹：逐行折入名字与目标，再折入拒绝计数。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(a) = self.rows.get(i) {
                h = fold_bytes(h, a.name.as_bytes());
                h = fold_byte(h, b'>');
                h = fold_bytes(h, a.target.as_bytes());
                h = fold_byte(h, b';');
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }
}

// ---------------------------------------------------------------------------
// 标准表与分派
// ---------------------------------------------------------------------------

/// 标准别名表（11 条，插入序故意打乱，靠 `add` 自己排序）。
///
/// 三条链是有意设计的：
/// - `zcat → gunzip → gzip`（2 跳，验证 HOP_CAP 边界内的转发）；
/// - `gunzip → gzip`、`uncompress → gzip`（1 跳，同族三个入口）；
/// - `halt/poweroff/reboot → init`（同一实现的三个电源入口）。
pub const ENTRIES: [(&str, &str); 11] = [
    ("gunzip", "gzip"),
    ("zcat", "gunzip"),
    ("[", "test"),
    ("bsdtar", "tar"),
    ("egrep", "grep"),
    ("fgrep", "grep"),
    ("halt", "init"),
    ("poweroff", "init"),
    ("reboot", "init"),
    ("sh", "ash"),
    ("uncompress", "gzip"),
];

/// 组装标准表。
pub fn standard_table() -> AliasTable {
    let mut t = AliasTable::new();
    let mut i = 0usize;
    while i < ENTRIES.len() {
        if let Some((n, tg)) = ENTRIES.get(i) {
            match Alias::new(n.as_bytes(), tg.as_bytes()) {
                Ok(a) => {
                    let _ = t.add(a);
                }
                Err(_) => {}
            }
        }
        i += 1;
    }
    t
}

/// 分派：`busybox` → Dispatcher；规范名 → Canonical；别名 → Alias；其余 Unknown。
pub fn dispatch(t: &AliasTable, raw: &[u8]) -> Result<Kind, u32> {
    let n = Name::new(raw)?;
    if eq_bytes(n.as_bytes(), b"busybox") {
        return Ok(Kind::Dispatcher);
    }
    if let Some(c) = canon_index(n.as_bytes()) {
        return Ok(Kind::Canonical(c));
    }
    match t.resolve(&n) {
        Some(h) => Ok(Kind::Alias { canon: h.canon, hops: h.hops }),
        None => Ok(Kind::Unknown),
    }
}

/// 安装期链接计划：规范名各一条 + 别名各一条。
pub fn install_links(t: &AliasTable) -> usize {
    t.len().saturating_add(CANON.len())
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

    /// 登记一格：槽位必须等于当前长度（严格顺序），冻结后一律拒。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(AL_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len = self.len.saturating_add(1);
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

    /// 对账：格数 / 成败计数 / 槽序 / 成功格值 / 失败格码 / 成功格错误码为 0。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(AL_EBAL);
        }
        const VALS: [u32; 8] = [10, 0, 2, 3, 11, 0xff, 10, 2];
        const CODES: [u32; 6] = [
            AL_EDUP,
            AL_ESHADOW,
            AL_EEMPTY,
            AL_ETARGET,
            AL_EBYTE,
            AL_ECAP,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(AL_EBAL),
            };
            if s.slot as usize != i {
                return Err(AL_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(AL_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(AL_EVALUE);
                        }
                    }
                    None => return Err(AL_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(AL_EVALUE);
                        }
                    }
                    None => return Err(AL_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(AL_EBAL);
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

    /// 冻结：记根，之后只读。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(AL_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放：重建同形账本，比对指纹。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let a = fresh.seal();
        if a == self.seal() {
            Ok(a)
        } else {
            Err(AL_ESEAL)
        }
    }

    /// 冻结根。
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

/// `Result<u32, u32>` → 样本。
fn cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 账本组装：8 成（查表/计数/形状/分派/链深）后 6 败（六类拒收）。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let t = standard_table();

    // 0: `[` → test（编号 10），1 跳。
    let _ = l.register(cell(
        0,
        OP_LOOKUP,
        match t.resolve_bytes(b"[") {
            Some(h) => Ok(h.canon as u32),
            None => Err(AL_ENOBS),
        },
    ));

    // 1: `sh` → ash（编号 0）。
    let _ = l.register(cell(
        1,
        OP_LOOKUP,
        match t.resolve_bytes(b"sh") {
            Some(h) => Ok(h.canon as u32),
            None => Err(AL_ENOBS),
        },
    ));

    // 2: `zcat` → gzip，跳数 2。
    let _ = l.register(cell(
        2,
        OP_LOOKUP,
        match t.resolve_bytes(b"zcat") {
            Some(h) => Ok(h.hops as u32),
            None => Err(AL_ENOBS),
        },
    ));

    // 3: 收敛到 gzip（编号 4）的别名共 3 条。
    let _ = l.register(cell(3, OP_COUNT, Ok(t.count_to(4) as u32)));

    // 4: 标准表别名条数 11。
    let _ = l.register(cell(
        4,
        OP_SHAPE,
        match t.shape() {
            Ok(s) => Ok(s.aliases as u32),
            Err(c) => Err(c),
        },
    ));

    // 5: `busybox` 自身 → 分派码 0xff。
    let _ = l.register(cell(
        5,
        OP_DISPATCH,
        match dispatch(&t, b"busybox") {
            Ok(k) => Ok(k.code()),
            Err(c) => Err(c),
        },
    ));

    // 6: 最长别名 `uncompress` 10 字节。
    let _ = l.register(cell(6, OP_LOOKUP, Ok(t.longest_alias() as u32)));

    // 7: 标准表最大链深 2。
    let _ = l.register(cell(
        7,
        OP_CHAIN,
        match t.shape() {
            Ok(s) => Ok(s.max_hops as u32),
            Err(c) => Err(c),
        },
    ));

    // 8: 重复别名 egrep。
    let _ = l.register(cell(
        8,
        OP_ADD,
        {
            let mut x = standard_table();
            match Alias::new(b"egrep", b"awk") {
                Ok(a) => x.add(a).map(|p| p as u32),
                Err(c) => Err(c),
            }
        },
    ));

    // 9: 遮蔽规范名 ls。
    let _ = l.register(cell(
        9,
        OP_ADD,
        {
            let mut x = standard_table();
            match Alias::new(b"ls", b"cat") {
                Ok(a) => x.add(a).map(|p| p as u32),
                Err(c) => Err(c),
            }
        },
    ));

    // 10: 空名。
    let _ = l.register(cell(
        10,
        OP_ADD,
        match Name::new(b"") {
            Ok(_) => Ok(0),
            Err(c) => Err(c),
        },
    ));

    // 11: 目标悬空。
    let _ = l.register(cell(
        11,
        OP_ADD,
        {
            let mut x = standard_table();
            match Alias::new(b"goner", b"nope") {
                Ok(a) => x.add(a).map(|p| p as u32),
                Err(c) => Err(c),
            }
        },
    ));

    // 12: 首字节非打印。
    let _ = l.register(cell(
        12,
        OP_ADD,
        match Name::new(&[0x01u8, b'x']) {
            Ok(_) => Ok(0),
            Err(c) => Err(c),
        },
    ));

    // 13: 表已满（12 槽装满后再插一条）。
    let _ = l.register(cell(
        13,
        OP_ADD,
        {
            let mut x = standard_table();
            let mut pre: Result<u32, u32> = Ok(0);
            match Alias::new(b"z64", b"gzip") {
                Ok(a) => {
                    let _ = x.add(a);
                }
                Err(c) => pre = Err(c),
            }
            match pre {
                Err(c) => Err(c),
                Ok(_) => match Alias::new(b"z65", b"gzip") {
                    Ok(a) => x.add(a).map(|p| p as u32),
                    Err(c) => Err(c),
                },
            }
        },
    ));

    l
}

// ---------------------------------------------------------------------------
// 判据面
// ---------------------------------------------------------------------------

/// 名闸：四类拒收 + 合法名文本往返。
fn name_gate_ok() -> bool {
    let a = matches!(Name::new(b"ls"), Ok(_));
    let b = Name::new(b"").err() == Some(AL_EEMPTY);
    let c = Name::new(&[b'a'; NAME_CAP + 1]).err() == Some(AL_ELEN);
    let d = Name::new(b"a/b").err() == Some(AL_ESLASH);
    let e = Name::new(&[0x01u8]).err() == Some(AL_EBYTE);
    let f = Name::new(b"a b").err() == Some(AL_EBYTE);
    let g = Name::new(&[b'a'; NAME_CAP]).is_ok();
    let h = match Name::new(b"zcat") {
        Ok(n) => n.len() == 4 && n.text().as_str() == "zcat" && !n.is_empty(),
        Err(_) => false,
    };
    a && b && c && d && e && f && g && h
}

/// 规范名表：条数 / 有序 / 六个精确编号 / 两个缺席。
fn canon_ok() -> bool {
    let a = CANON.len() == 12;
    let b = canon_sorted();
    let c = canon_index(b"ash") == Some(0);
    let d = canon_index(b"test") == Some(10);
    let e = canon_index(b"gzip") == Some(4);
    let f = canon_index(b"vi") == Some(11);
    let g = canon_index(b"less").is_none();
    let h = canon_index(b"").is_none();
    let i = canon_index(b"AS") .is_none();
    a && b && c && d && e && f && g && h && i
}

/// 表形状：11 别名 / 12 规范 / 2 跳 / 23 链接。
fn table_shape_ok() -> bool {
    let t = standard_table();
    let s = match t.shape() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if s.aliases != 11 || s.canonicals != 12 || s.max_hops != 2 || s.links != 23 {
        return false;
    }
    if t.len() != 11 || t.is_empty() || t.rejects() != 0 {
        return false;
    }
    let row = match t.find(b"sh") {
        Some(a) => a.target.text().as_str() == "ash" && a.text().as_str() == "sh->ash",
        None => return false,
    };
    row
        && t.find(b"nope").is_none()
        && t.find(b"busybox").is_none()
        && install_links(&t) == 23
}

/// 有序插入：故意乱序插入，行序仍严格递增，返回位置正确。
fn sorted_ok() -> bool {
    let mut t = AliasTable::new();
    let a = match Alias::new(b"zz", b"ls") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let b = match Alias::new(b"aa", b"cat") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let c = match Alias::new(b"mm", b"ps") {
        Ok(v) => v,
        Err(_) => return false,
    };
    if t.add(a) != Ok(0) {
        return false;
    }
    if t.add(b) != Ok(0) {
        return false;
    }
    if t.add(c) != Ok(1) {
        return false;
    }
    if !t.is_sorted() || t.len() != 3 {
        return false;
    }
    match (t.at(0), t.at(1), t.at(2)) {
        (Some(x), Some(y), Some(z)) => {
            x.name.text().as_str() == "aa"
                && y.name.text().as_str() == "mm"
                && z.name.text().as_str() == "zz"
        }
        _ => false,
    }
}

/// 解析：0/1/2 跳全覆盖，未知与分派名一律不命中。
fn resolve_ok() -> bool {
    let t = standard_table();
    let a = t.resolve_bytes(b"ls") == Some(Hit { canon: 6, hops: 0 });
    let b = t.resolve_bytes(b"[") == Some(Hit { canon: 10, hops: 1 });
    let c = t.resolve_bytes(b"bsdtar") == Some(Hit { canon: 9, hops: 1 });
    let d = t.resolve_bytes(b"sh") == Some(Hit { canon: 0, hops: 1 });
    let e = t.resolve_bytes(b"zcat") == Some(Hit { canon: 4, hops: 2 });
    let f = t.resolve_bytes(b"nope").is_none();
    let g = t.resolve_bytes(b"busybox").is_none();
    let h = resolve_in(&t.rows, t.len, &nm(b"uncompress")) == Some(Hit { canon: 4, hops: 1 });
    a && b && c && d && e && f && g && h
}

/// 重复别名：拒、计数加一、表长不变、原目标不被改写。
fn dup_ok() -> bool {
    let mut t = standard_table();
    let before = t.len();
    let r = match Alias::new(b"egrep", b"awk") {
        Ok(a) => t.add(a),
        Err(c) => return c == AL_EDUP,
    };
    if r != Err(AL_EDUP) || t.len() != before || t.rejects() != 1 {
        return false;
    }
    match t.find(b"egrep") {
        Some(a) => a.target.text().as_str() == "grep",
        None => false,
    }
}

/// 遮蔽：别名名撞规范名一律拒，不论目标。
fn shadow_ok() -> bool {
    let mut t = standard_table();
    let a = match Alias::new(b"ls", b"cat") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let r1 = t.add(a);
    let b = match Alias::new(b"cat", b"grep") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let r2 = t.add(b);
    let c = match Alias::new(b"test", b"test2") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let r3 = t.add(c);
    r1 == Err(AL_ESHADOW) && r2 == Err(AL_ESHADOW) && r3 == Err(AL_ESHADOW) && t.rejects() == 3
}

/// 链深：3 跳链可装入（装时每步都 ≤ 2），但整表形状闸判越界。
fn chain_ok() -> bool {
    let mut t = AliasTable::new();
    let order: [(&[u8], &[u8]); 3] = [(b"z3", b"gzip"), (b"z2", b"z3"), (b"z1", b"z2")];
    let mut i = 0usize;
    while i < order.len() {
        match order.get(i) {
            Some((n, g)) => match Alias::new(n, g) {
                Ok(a) => {
                    if t.add(a).is_err() {
                        return false;
                    }
                }
                Err(_) => return false,
            },
            None => return false,
        }
        i += 1;
    }
    if t.len() != 3 {
        return false;
    }
    let deep = t.resolve_bytes(b"z1").is_none();
    let two = t.resolve_bytes(b"z2") == Some(Hit { canon: 4, hops: 2 });
    let gate = t.shape() == Err(AL_ECAP);
    let self_alias = Alias::new(b"q", b"q").err() == Some(AL_EDUP);
    deep && two && gate && self_alias
}

/// 分派四态：编码 + 标签 + 内建位。
fn dispatch_ok() -> bool {
    let t = standard_table();
    let a = dispatch(&t, b"busybox") == Ok(Kind::Dispatcher);
    let b = dispatch(&t, b"ps") == Ok(Kind::Canonical(7));
    let c = dispatch(&t, b"sh") == Ok(Kind::Alias { canon: 0, hops: 1 });
    let d = dispatch(&t, b"zzz") == Ok(Kind::Unknown);
    let e = dispatch(&t, b"") == Err(AL_EEMPTY);
    let f = dispatch(&t, b"a/b") == Err(AL_ESLASH);
    let g = dispatch(&t, &[b'q'; NAME_CAP + 1]) == Err(AL_ELEN);
    let h = Kind::Dispatcher.code() == 0xff
        && Kind::Unknown.code() == 0xee
        && Kind::Canonical(3).code() == 0x13
        && Kind::Alias { canon: 0, hops: 1 }.code() == 0x21
        && Kind::Alias { canon: 4, hops: 2 }.code() == 0x32;
    a && b && c && d && e && f && g && h
}

/// 计数与极值：按目标归并、最长别名、最大跳数。
fn count_ok() -> bool {
    let t = standard_table();
    let a = t.count_to(4) == 3;
    let b = t.count_to(3) == 2;
    let c = t.count_to(0) == 1;
    let d = t.count_to(6) == 0;
    let e = t.count_to(255) == 0;
    let f = t.longest_alias() == 10;
    let g = match t.shape() {
        Ok(s) => s.max_hops == 2,
        Err(_) => false,
    };
    a && b && c && d && e && f && g
}

/// 指纹：同构同值、加一条即变、拒绝也进指纹。
fn seal_ok() -> bool {
    let a = standard_table();
    let b = standard_table();
    if a.seal() != b.seal() || a.seal() == 0 {
        return false;
    }
    let mut c = standard_table();
    match Alias::new(b"z64", b"gzip") {
        Ok(x) => {
            if c.add(x) != Ok(10) {
                return false;
            }
        }
        Err(_) => return false,
    }
    if c.seal() == a.seal() {
        return false;
    }
    let mut d = standard_table();
    let _ = d.add(match Alias::new(b"egrep", b"awk") {
        Ok(v) => v,
        Err(_) => return false,
    });
    d.seal() != a.seal() && d.rejects() == 1
}

/// 容量：12 槽装满，第 13 条拒收，长度与拒绝计数同步。
fn cap_ok() -> bool {
    let mut t = standard_table();
    let x = match Alias::new(b"z64", b"gzip") {
        Ok(v) => v,
        Err(_) => return false,
    };
    if t.add(x) != Ok(10) {
        return false;
    }
    if t.len() != ALIAS_CAP {
        return false;
    }
    let y = match Alias::new(b"z65", b"gzip") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let r = t.add(y);
    r == Err(AL_ECAP) && t.rejects() == 1 && t.len() == ALIAS_CAP
}

/// 账本闭环：对账 + 指纹稳定 + 冻结重放。
fn ledger_ok() -> bool {
    let l = build_ledger();
    if l.audit() != Ok(()) {
        return false;
    }
    if l.len() != SAMPLES || l.ok_count() != 8 || l.err_count() != 6 {
        return false;
    }
    if l.seal() != build_ledger().seal() {
        return false;
    }
    let mut l2 = build_ledger();
    let root = match l2.freeze() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if !l2.is_frozen() || l2.root() != root {
        return false;
    }
    if l2.replay() != Ok(root) {
        return false;
    }
    if l2.freeze() != Err(AL_EBAL) {
        return false;
    }
    l2.rejects() == 1
}

/// 四路注入：名闸+规范名 / 形状+有序+解析 / 三类拒收 / 分派+计数+指纹+容量+账本。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    if let Some(dst) = out.get_mut(0) {
        *dst = name_gate_ok() && canon_ok();
    }
    if let Some(dst) = out.get_mut(1) {
        *dst = table_shape_ok() && sorted_ok() && resolve_ok();
    }
    if let Some(dst) = out.get_mut(2) {
        *dst = dup_ok() && shadow_ok() && chain_ok();
    }
    if let Some(dst) = out.get_mut(3) {
        *dst = dispatch_ok() && count_ok() && seal_ok() && cap_ok() && ledger_ok();
    }
    out
}

/// 判据集合（13 项）。
pub fn run_unx11593_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11593.alias");

    s.add("AL01-名闸四类", name_gate_ok(), "static");
    s.add("AL02-规范名表", canon_ok(), "static");
    s.add("AL03-别名表形状", table_shape_ok(), "static");
    s.add("AL04-有序插入", sorted_ok(), "static");
    s.add("AL05-别名解析", resolve_ok(), "static");
    s.add("AL06-重复别名", dup_ok(), "static");
    s.add("AL07-遮蔽规范名", shadow_ok(), "static");
    s.add("AL08-链深越界", chain_ok(), "static");
    s.add("AL09-分派四态", dispatch_ok(), "static");
    s.add("AL10-归并计数", count_ok(), "static");
    s.add("AL11-别名表指纹", seal_ok(), "static");
    s.add("AL12-账本对账", build_ledger().audit() == Ok(()), "static");
    let four = inject_four();
    s.add("AL13-冻结重放四注入", four[0] && four[1] && four[2] && four[3], "static");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_gate_matrix() {
        assert!(Name::new(b"ls").is_ok());
        assert_eq!(Name::new(b"").err(), Some(AL_EEMPTY));
        assert_eq!(Name::new(&[b'a'; NAME_CAP + 1]).err(), Some(AL_ELEN));
        assert_eq!(Name::new(b"a/b").err(), Some(AL_ESLASH));
        assert_eq!(Name::new(&[0x01]).err(), Some(AL_EBYTE));
        assert_eq!(nm(b"zcat").text().as_str(), "zcat");
    }

    #[test]
    fn canon_table_order() {
        assert_eq!(CANON.len(), 12);
        assert!(canon_sorted());
        assert_eq!(canon_index(b"ash"), Some(0));
        assert_eq!(canon_index(b"test"), Some(10));
        assert_eq!(canon_index(b"gzip"), Some(4));
        assert_eq!(canon_index(b"less"), None);
        assert_eq!(canon_name(11), "vi");
        assert_eq!(canon_name(12), "");
    }

    #[test]
    fn standard_shape() {
        let t = standard_table();
        let s = t.shape().expect("shape");
        assert_eq!(s.aliases, 11);
        assert_eq!(s.canonicals, 12);
        assert_eq!(s.max_hops, 2);
        assert_eq!(s.links, 23);
        assert_eq!(t.len(), 11);
        assert_eq!(install_links(&t), 23);
    }

    #[test]
    fn sorted_insert_positions() {
        let mut t = AliasTable::new();
        assert_eq!(t.add(Alias::new(b"zz", b"ls").expect("a")), Ok(0));
        assert_eq!(t.add(Alias::new(b"aa", b"cat").expect("b")), Ok(0));
        assert_eq!(t.add(Alias::new(b"mm", b"ps").expect("c")), Ok(1));
        assert!(t.is_sorted());
        assert_eq!(t.at(1).map(|a| a.name.text()), Some(String::from("mm")));
        assert_eq!(t.at(3), None);
    }

    #[test]
    fn resolve_hops() {
        let t = standard_table();
        assert_eq!(t.resolve_bytes(b"ls"), Some(Hit { canon: 6, hops: 0 }));
        assert_eq!(t.resolve_bytes(b"["), Some(Hit { canon: 10, hops: 1 }));
        assert_eq!(t.resolve_bytes(b"zcat"), Some(Hit { canon: 4, hops: 2 }));
        assert_eq!(t.resolve_bytes(b"nope"), None);
    }

    #[test]
    fn duplicate_alias() {
        let mut t = standard_table();
        assert_eq!(t.add(Alias::new(b"egrep", b"awk").expect("d")), Err(AL_EDUP));
        assert_eq!(t.len(), 11);
        assert_eq!(t.rejects(), 1);
        assert_eq!(t.find(b"egrep").map(|a| a.target.text()), Some(String::from("grep")));
    }

    #[test]
    fn shadow_canonical() {
        let mut t = standard_table();
        assert_eq!(t.add(Alias::new(b"ls", b"cat").expect("s")), Err(AL_ESHADOW));
        assert_eq!(t.add(Alias::new(b"tar", b"gzip").expect("t")), Err(AL_ESHADOW));
        assert_eq!(t.rejects(), 2);
    }

    #[test]
    fn deep_chain_rejected() {
        let mut t = AliasTable::new();
        let _ = t.add(Alias::new(b"z3", b"gzip").expect("1"));
        let _ = t.add(Alias::new(b"z2", b"z3").expect("2"));
        assert_eq!(t.add(Alias::new(b"z1", b"z2").expect("3")), Ok(0));
        assert_eq!(t.resolve_bytes(b"z1"), None);
        assert_eq!(t.resolve_bytes(b"z2"), Some(Hit { canon: 4, hops: 2 }));
        assert_eq!(t.shape(), Err(AL_ECAP));
        assert_eq!(Alias::new(b"q", b"q").err(), Some(AL_EDUP));
    }

    #[test]
    fn dispatch_codes() {
        let t = standard_table();
        assert_eq!(dispatch(&t, b"busybox"), Ok(Kind::Dispatcher));
        assert_eq!(dispatch(&t, b"ps"), Ok(Kind::Canonical(7)));
        assert_eq!(dispatch(&t, b"sh"), Ok(Kind::Alias { canon: 0, hops: 1 }));
        assert_eq!(dispatch(&t, b"zzz"), Ok(Kind::Unknown));
        assert_eq!(dispatch(&t, b""), Err(AL_EEMPTY));
        assert_eq!(Kind::Dispatcher.code(), 0xff);
        assert_eq!(Kind::Alias { canon: 4, hops: 2 }.code(), 0x32);
        assert!(Kind::Dispatcher.is_builtin() && !Kind::Unknown.is_known());
    }

    #[test]
    fn counts_by_target() {
        let t = standard_table();
        assert_eq!(t.count_to(4), 3);
        assert_eq!(t.count_to(3), 2);
        assert_eq!(t.count_to(0), 1);
        assert_eq!(t.count_to(6), 0);
        assert_eq!(t.longest_alias(), 10);
    }

    #[test]
    fn seal_changes_on_add() {
        let a = standard_table();
        let b = standard_table();
        assert_eq!(a.seal(), b.seal());
        let mut c = standard_table();
        let _ = c.add(Alias::new(b"z64", b"gzip").expect("z"));
        assert!(c.seal() != a.seal());
    }

    #[test]
    fn capacity_limit() {
        let mut t = standard_table();
        assert_eq!(t.add(Alias::new(b"z64", b"gzip").expect("x")), Ok(10));
        assert_eq!(t.add(Alias::new(b"z65", b"gzip").expect("y")), Err(AL_ECAP));
        assert_eq!(t.len(), ALIAS_CAP);
        assert_eq!(t.rejects(), 1);
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.sample(0).map(|s| s.value), Some(10));
        assert_eq!(l.sample(5).map(|s| s.value), Some(0xff));
        assert_eq!(l.sample(8).map(|s| s.code), Some(AL_EDUP));
        assert_eq!(l.sample(13).map(|s| s.code), Some(AL_ECAP));
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = l.freeze().expect("freeze");
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(AL_EBAL));
        assert_eq!(l.rejects(), 1);
        assert!(l.is_frozen());
        assert_eq!(l.root(), root);
    }

    #[test]
    fn injections() {
        assert_eq!(inject_four(), [true, true, true, true]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11593_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
