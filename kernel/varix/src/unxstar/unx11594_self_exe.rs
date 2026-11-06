//! UNX-F11593 /proc/self/exe 自路径判据
//!
//! `/proc/self/exe` 是内核给进程开的一扇「我是谁」的窗：读出来是一条
//! 指向自身可执行文件的符号链接目标。它看着只是一次 `readlink`，实际
//! 要钉住五层语义：
//!
//! ```text
//! 归一   `//` 折叠、`.` 丢弃、`..` 退段（退到根外即拒）、尾斜杠丢弃
//! 根     归一结果恒以 `/` 开头；空串（根）归一为 `/`，绝不裸空
//! 回退   链接为空时按 argv[0]：绝对路径直接用；相对名走 PATH 逐目录检索
//! 删除   目标被 unlink 后内核追加 `" (deleted)"`，必须剥离并置位
//! 同一   「同一可执行文件」= 归一后的字节串相等，不是字面相等
//! ```
//!
//! 为什么归一要先于回退：argv[0] 可能是 `./busybox`、`/bin//busybox`、
//! `/bin/../bin/busybox` 三种写法，只有先把它们折成同一条规范串，`identity`
//! 才能拿它和链接目标做字节比较——否则「同一个文件」会被判成两个。
//!
//! 本模块把路径做成**定容字节串**（96 字节，最多 10 段，单段 32 字节），
//! 归一过程不分配、不回溯、不越界：段缓冲固定 32 字节，写入前先查剩余
//! 容量，`..` 退段靠反向扫描最后一个 `/`，根的特例（`len == 1`）单独
//! 处理，避免把 `/a` 退成空串。
//!
//! PATH 检索是对**常量文件系统表** `FS` 的纯模型：只有表内 6 条真实存在，
//! 其余候选一律视为不存在。这样「检索顺序」「命中哪一条」都可手算，不依赖
//! 宿主环境，也不引入任何 I/O。
//!
//! 闸门次序：空 → 长度 → 字节 → 绝对性 → 点段 → 段长 → 段数 → 目标存在。
//!
//! 账本 14 格（8 成 6 败）在 `audit` 逐格对账；冻结后重放须同指纹。
//! 无 std、无 unsafe、无 panic、无 unwrap。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（本卡专属）。
pub const SEG_BASE: u32 = 0xfa70;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 整条路径的字节容量。
pub const PATH_CAP: usize = 96;

/// 单段字节容量。
pub const SEG_CAP: usize = 32;

/// 最多段数。
pub const MAX_SEGS: usize = 10;

/// 被删除文件的后缀（内核 `d_path` 追加）。
pub const DELETED_SUFFIX: &[u8] = b" (deleted)";

/// 常量文件系统表：只有这些路径「存在」。
pub const FS: [&str; 6] = [
    "/bin/busybox",
    "/usr/bin/busybox",
    "/bin/sh",
    "/sbin/init",
    "/usr/local/bin/busybox",
    "/bin/ls",
];

/// 默认 PATH（容器里最常见的两段）。
pub const DEFAULT_PATH: &[u8] = b"/usr/local/bin:/bin";

// ---------------------------------------------------------------------------
// 错误码：8 个域错误 + 5 个账本错误
// ---------------------------------------------------------------------------

/// 路径含控制字节（< 0x20 或 0x7f）。
pub const SX_EBYTE: u32 = SEG_BASE + 0x01;
/// 路径超长（> PATH_CAP）。
pub const SX_ELEN: u32 = SEG_BASE + 0x02;
/// 空路径（既无链接也无 argv[0] 时也归此族的近亲 ENOENT）。
pub const SX_EMPTY: u32 = SEG_BASE + 0x03;
/// 相对路径（归一要求绝对）。
pub const SX_EREL: u32 = SEG_BASE + 0x04;
/// `..` 退到根之外。
pub const SX_EDOT: u32 = SEG_BASE + 0x05;
/// 单段超长（> SEG_CAP）。
pub const SX_ESEG: u32 = SEG_BASE + 0x06;
/// 段数超限（> MAX_SEGS）。
pub const SX_EDEEP: u32 = SEG_BASE + 0x07;
/// 检索不到（PATH 全空 / 候选全不存在 / 无链接且无 argv[0]）。
pub const SX_ENOENT: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const SX_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const SX_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const SX_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const SX_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const SX_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 归一。
pub const OP_NORM: u8 = 0x01;
/// 读链接。
pub const OP_READLINK: u8 = 0x02;
/// 回退解析。
pub const OP_FALLBACK: u8 = 0x03;
/// 同一判定。
pub const OP_IDENT: u8 = 0x04;
/// 删除后缀。
pub const OP_STRIP: u8 = 0x05;
/// PATH 候选。
pub const OP_SEARCH: u8 = 0x06;

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

/// 前缀相等（长度不足视为否）。
fn starts_with(a: &[u8], p: &[u8]) -> bool {
    if a.len() < p.len() {
        return false;
    }
    let mut i = 0usize;
    while i < p.len() {
        match (a.get(i), p.get(i)) {
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

/// 后缀相等。
fn ends_with(a: &[u8], s: &[u8]) -> bool {
    if a.len() < s.len() {
        return false;
    }
    let at = a.len() - s.len();
    let mut i = 0usize;
    while i < s.len() {
        match (a.get(at + i), s.get(i)) {
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

/// 十进制拼进 String。
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
// 路径
// ---------------------------------------------------------------------------

/// 归一后的定容路径。
///
/// 不变量：`1 <= len <= PATH_CAP`；首字节恒 `/`；不含空段、`.` 段、`..` 段；
/// 除根（`len == 1`）外不以 `/` 结尾。
///
/// 相等性**只比较有效字节**：缓冲区尾部是被 `len` 遮住的残留字节（插入
/// 段、退段都会留下旧值），派生比较会把它们算进去，从而把两条同路径判成
/// 不同——所以这里手写 `PartialEq`，语义与 `as_bytes()` 严格一致。
#[derive(Clone, Copy, Debug)]
pub struct Path {
    buf: [u8; PATH_CAP],
    len: u8,
}

impl PartialEq for Path {
    fn eq(&self, other: &Path) -> bool {
        eq_bytes(self.as_bytes(), other.as_bytes())
    }
}

impl Eq for Path {}

impl Path {
    /// 零路径（仅作表槽占位，失败路径才会出现）。
    pub const fn empty() -> Path {
        Path { buf: [0u8; PATH_CAP], len: 0 }
    }

    /// 根 `/`。
    pub fn root() -> Path {
        let mut p = Path::empty();
        p.push(b'/');
        p
    }

    /// 视图（长度夹紧）。
    pub fn as_bytes(&self) -> &[u8] {
        let n = (self.len as usize).min(PATH_CAP);
        match self.buf.get(..n) {
            Some(s) => s,
            None => &[],
        }
    }

    /// 字节长度。
    pub fn len(&self) -> usize {
        self.as_bytes().len()
    }

    /// 是否空路径。
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 是否绝对。
    pub fn is_absolute(&self) -> bool {
        match self.as_bytes().get(0) {
            Some(b) => *b == b'/',
            None => false,
        }
    }

    /// 是否根。
    pub fn is_root(&self) -> bool {
        self.len() == 1 && self.as_bytes().get(0).copied() == Some(b'/')
    }

    /// 追加一个字节（容量不足静默丢弃；调用方应先查容量）。
    fn push(&mut self, b: u8) {
        let at = self.len as usize;
        if at < PATH_CAP {
            if let Some(dst) = self.buf.get_mut(at) {
                *dst = b;
                self.len = self.len.saturating_add(1);
            }
        }
    }

    /// 追加一段（自动补分隔斜杠；根之后不补双斜杠）。
    fn push_seg(&mut self, s: &[u8]) -> Result<(), u32> {
        if self.len() > 1 {
            self.push(b'/');
        }
        if self.len().saturating_add(s.len()) > PATH_CAP {
            return Err(SX_ELEN);
        }
        let mut i = 0usize;
        while i < s.len() {
            if let Some(b) = s.get(i) {
                self.push(*b);
            }
            i += 1;
        }
        Ok(())
    }

    /// 退一段：反向找最后一个 `/`；退到根时保留 `/`。
    fn pop_seg(&mut self) {
        let mut i = self.len as usize;
        while i > 0 {
            i -= 1;
            let at = i;
            let b = self.buf.get(at).copied().unwrap_or(0);
            if b == b'/' {
                self.len = if at == 0 { 1 } else { at as u8 };
                return;
            }
        }
        self.len = 0;
    }

    /// 段数（根为 0）。
    pub fn depth(&self) -> usize {
        if self.is_root() || self.is_empty() {
            return 0;
        }
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len() {
            if self.as_bytes().get(i).copied() == Some(b'/') {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 基名（根的回 `/`）。
    pub fn basename(&self) -> &[u8] {
        let b = self.as_bytes();
        if self.is_root() {
            return b;
        }
        let mut i = b.len();
        while i > 0 {
            i -= 1;
            match b.get(i) {
                Some(x) => {
                    if *x == b'/' {
                        return match b.get(i + 1..) {
                            Some(s) => s,
                            None => b,
                        };
                    }
                }
                None => break,
            }
        }
        b
    }

    /// 目录部分（根的回根）。
    pub fn dirname(&self) -> Path {
        let mut p = *self;
        if p.is_root() || p.is_empty() {
            return Path::root();
        }
        p.pop_seg();
        if p.is_empty() {
            return Path::root();
        }
        p
    }

    /// 段级前缀归属：`/usr/bin` 是 `/usr/bin/ls` 的祖先，但不是 `/usr/binge` 的。
    pub fn is_under(&self, dir: &Path) -> bool {
        let p = self.as_bytes();
        let d = dir.as_bytes();
        if d.is_empty() || p.is_empty() {
            return false;
        }
        if !starts_with(p, d) {
            return false;
        }
        if d.len() == p.len() {
            return true;
        }
        if dir.is_root() {
            return true;
        }
        p.get(d.len()).copied() == Some(b'/')
    }

    /// 文本。
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

    /// 指纹。
    pub fn seal(&self) -> u64 {
        fold_bytes(FNV_OFF, self.as_bytes())
    }
}

// ---------------------------------------------------------------------------
// 归一
// ---------------------------------------------------------------------------

/// 归一：折叠 `//`、丢 `.`、`..` 退段、丢尾斜杠；根恒为 `/`。
///
/// 闸门次序不可换：先空/长/字节（这四条与结构无关，越早越省），再绝对性，
/// 再逐段处理（点段 → 段长 → 段数 → 容量）。
pub fn normalize(raw: &[u8]) -> Result<Path, u32> {
    if raw.is_empty() {
        return Err(SX_EMPTY);
    }
    if raw.len() > PATH_CAP {
        return Err(SX_ELEN);
    }
    let mut i = 0usize;
    while i < raw.len() {
        match raw.get(i) {
            Some(b) => {
                if *b < 0x20 || *b > 0x7e {
                    return Err(SX_EBYTE);
                }
            }
            None => return Err(SX_EBYTE),
        }
        i += 1;
    }
    if raw.get(0).copied() != Some(b'/') {
        return Err(SX_EREL);
    }

    let mut out = Path::root();
    let mut seg = [0u8; SEG_CAP];
    let mut sl = 0usize;
    let mut segs = 0usize;
    let mut at = 0usize;
    while at <= raw.len() {
        let b = raw.get(at).copied().unwrap_or(b'/');
        if b == b'/' {
            if sl > 0 {
                let s = match seg.get(..sl) {
                    Some(v) => v,
                    None => return Err(SX_ESEG),
                };
                if eq_bytes(s, b".") {
                    // 当前目录：丢弃
                } else if eq_bytes(s, b"..") {
                    if out.is_root() {
                        return Err(SX_EDOT);
                    }
                    out.pop_seg();
                } else {
                    segs = segs.saturating_add(1);
                    if segs > MAX_SEGS {
                        return Err(SX_EDEEP);
                    }
                    out.push_seg(s)?;
                }
                sl = 0;
            }
        } else {
            if sl >= SEG_CAP {
                return Err(SX_ESEG);
            }
            if let Some(dst) = seg.get_mut(sl) {
                *dst = b;
            }
            sl = sl.saturating_add(1);
        }
        at += 1;
    }
    Ok(out)
}

/// 同一可执行文件：两者都归一成功且字节相等。
pub fn identity(a: &[u8], b: &[u8]) -> bool {
    match (normalize(a), normalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// 删除后缀
// ---------------------------------------------------------------------------

/// 拆分 `" (deleted)"` 后缀：返回（剥离后的字节，是否删除）。
pub fn split_deleted(raw: &[u8]) -> (&[u8], bool) {
    if ends_with(raw, DELETED_SUFFIX) {
        let at = raw.len().saturating_sub(DELETED_SUFFIX.len());
        match raw.get(..at) {
            Some(s) => (s, true),
            None => (raw, false),
        }
    } else {
        (raw, false)
    }
}

// ---------------------------------------------------------------------------
// 来源与可执行体
// ---------------------------------------------------------------------------

/// 路径来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// 来自 `/proc/self/exe` 链接。
    Link,
    /// 来自绝对 argv[0]。
    Argv0,
    /// 来自 PATH 检索。
    PathSearch,
}

impl Source {
    /// 来源编码（1/2/3）。
    pub fn code(&self) -> u32 {
        match self {
            Source::Link => 1,
            Source::Argv0 => 2,
            Source::PathSearch => 3,
        }
    }

    /// 标签。
    pub fn tag(&self) -> &'static str {
        match self {
            Source::Link => "link",
            Source::Argv0 => "argv0",
            Source::PathSearch => "path",
        }
    }
}

/// 自路径解析结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Exe {
    /// 归一后的路径。
    pub path: Path,
    /// 来源。
    pub source: Source,
    /// 目标是否已被删除（内核追加过后缀）。
    pub deleted: bool,
}

impl Exe {
    /// 是否已删除。
    pub fn is_deleted(&self) -> bool {
        self.deleted
    }

    /// 诊断文本 `path (source[,deleted])`。
    pub fn text(&self) -> String {
        let mut s = self.path.text();
        s.push_str(" [");
        s.push_str(self.source.tag());
        if self.deleted {
            s.push_str(",deleted");
        }
        s.push(']');
        s
    }

    /// 指纹：路径字节 + 来源 + 删除位。
    pub fn seal(&self) -> u64 {
        let h = fold_bytes(FNV_OFF, self.path.as_bytes());
        let h = fold_byte(h, self.source.code() as u8);
        fold_byte(h, self.deleted as u8)
    }
}

// ---------------------------------------------------------------------------
// 读链接
// ---------------------------------------------------------------------------

/// 读 `/proc/self/exe`：空目标 → ENOENT；剥删除后缀后归一。
pub fn readlink_exe(link: &[u8]) -> Result<Exe, u32> {
    if link.is_empty() {
        return Err(SX_ENOENT);
    }
    let (raw, deleted) = split_deleted(link);
    if raw.is_empty() {
        return Err(SX_EMPTY);
    }
    let p = normalize(raw)?;
    Ok(Exe { path: p, source: Source::Link, deleted })
}

/// 常量文件系统里是否存在该路径（逐条归一比较）。
pub fn exists(p: &Path) -> bool {
    let mut i = 0usize;
    while i < FS.len() {
        if let Some(t) = FS.get(i) {
            match normalize(t.as_bytes()) {
                Ok(q) => {
                    if q == *p {
                        return true;
                    }
                }
                Err(_) => {}
            }
        }
        i += 1;
    }
    false
}

/// 把 `dir` 与 `name` 拼进定容缓冲，返回有效长度。
fn join_into(dir: &[u8], name: &[u8], buf: &mut [u8]) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < dir.len() {
        if let (Some(dst), Some(src)) = (buf.get_mut(n), dir.get(i)) {
            *dst = *src;
            n = n.saturating_add(1);
        }
        i += 1;
    }
    if n > 0 {
        if let Some(dst) = buf.get_mut(n) {
            *dst = b'/';
            n = n.saturating_add(1);
        }
    }
    let mut k = 0usize;
    while k < name.len() {
        if let (Some(dst), Some(src)) = (buf.get_mut(n), name.get(k)) {
            *dst = *src;
            n = n.saturating_add(1);
        }
        k += 1;
    }
    n
}

/// PATH 候选数（空串段跳过——不做「空段即当前目录」的隐式行为）。
pub fn candidates(_name: &[u8], path_env: &[u8]) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    let mut start = 0usize;
    while i <= path_env.len() {
        let b = path_env.get(i).copied().unwrap_or(b':');
        if b == b':' {
            if i > start {
                n = n.saturating_add(1);
            }
            start = i.saturating_add(1);
        }
        i += 1;
    }
    n
}

/// 在 PATH 中检索 argv[0]：逐目录拼接 → 归一 → 查常量表，首个命中即返回。
pub fn search_path(argv0: &[u8], path_env: &[u8]) -> Result<Path, u32> {
    if argv0.is_empty() {
        return Err(SX_ENOENT);
    }
    if argv0.get(0).copied() == Some(b'/') {
        return normalize(argv0);
    }
    let mut buf = [0u8; PATH_CAP];
    let mut i = 0usize;
    let mut start = 0usize;
    while i <= path_env.len() {
        let b = path_env.get(i).copied().unwrap_or(b':');
        if b == b':' {
            if i > start {
                if let Some(dir) = path_env.get(start..i) {
                    let n = join_into(dir, argv0, &mut buf);
                    if n > 0 && n <= PATH_CAP {
                        if let Some(raw) = buf.get(..n) {
                            match normalize(raw) {
                                Ok(p) => {
                                    if exists(&p) {
                                        return Ok(p);
                                    }
                                }
                                Err(_) => {}
                            }
                        }
                    }
                }
            }
            start = i.saturating_add(1);
        }
        i += 1;
    }
    Err(SX_ENOENT)
}

/// 自路径总解析：链接优先，其次 argv[0]，最后 PATH。
///
/// 注意链接非空但损坏（相对、越根、超长）时**直接报错**，不回退——这是
/// 判据的关键：`/proc/self/exe` 明明读到了值却解析失败，说明内核/挂载面
/// 异常，静默回退会把「异常」伪装成「正常工作」。
pub fn resolve_exe(link: &[u8], argv0: &[u8], path_env: &[u8]) -> Result<Exe, u32> {
    if !link.is_empty() {
        return readlink_exe(link);
    }
    if argv0.is_empty() {
        return Err(SX_ENOENT);
    }
    if argv0.get(0).copied() == Some(b'/') {
        let p = normalize(argv0)?;
        return Ok(Exe { path: p, source: Source::Argv0, deleted: false });
    }
    let p = search_path(argv0, path_env)?;
    Ok(Exe { path: p, source: Source::PathSearch, deleted: false })
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

    /// 登记（严格槽序，冻结后拒）。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(SX_ESLOT);
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

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(SX_EBAL);
        }
        const VALS: [u32; 8] = [16, 4, 7, 1, 3, 6, 1, 3];
        const CODES: [u32; 6] = [
            SX_EMPTY,
            SX_EREL,
            SX_EDOT,
            SX_ELEN,
            SX_ESEG,
            SX_ENOENT,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(SX_EBAL),
            };
            if s.slot as usize != i {
                return Err(SX_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(SX_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(SX_EVALUE);
                        }
                    }
                    None => return Err(SX_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(SX_EVALUE);
                        }
                    }
                    None => return Err(SX_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(SX_EBAL);
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
            return Err(SX_EBAL);
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
            Err(SX_ESEAL)
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

/// 构造定长全 `a` 路径（首字节 `/`）。
fn padded(n: usize) -> [u8; PATH_CAP + 8] {
    let mut buf = [b'a'; PATH_CAP + 8];
    if let Some(dst) = buf.get_mut(0) {
        *dst = b'/';
    }
    let _ = n;
    buf
}

/// 账本组装：8 成 6 败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    // 0: `//` 与 `.` 折叠后的长度 16。
    let _ = l.register(cell(
        0,
        OP_NORM,
        match normalize(b"/usr//bin/./busybox") {
            Ok(p) => Ok(p.len() as u32),
            Err(c) => Err(c),
        },
    ));

    // 1: 段数 4。
    let _ = l.register(cell(
        1,
        OP_NORM,
        match normalize(b"/usr/local/bin/busybox") {
            Ok(p) => Ok(p.depth() as u32),
            Err(c) => Err(c),
        },
    ));

    // 2: 基名 `busybox` 7 字节。
    let _ = l.register(cell(
        2,
        OP_NORM,
        match normalize(b"/usr/local/bin/busybox") {
            Ok(p) => Ok(p.basename().len() as u32),
            Err(c) => Err(c),
        },
    ));

    // 3: 链接来源编码 1。
    let _ = l.register(cell(
        3,
        OP_READLINK,
        match readlink_exe(b"/usr/bin/busybox") {
            Ok(e) => Ok(e.source.code()),
            Err(c) => Err(c),
        },
    ));

    // 4: PATH 检索来源编码 3。
    let _ = l.register(cell(
        4,
        OP_FALLBACK,
        match resolve_exe(b"", b"busybox", DEFAULT_PATH) {
            Ok(e) => Ok(e.source.code()),
            Err(c) => Err(c),
        },
    ));

    // 5: 剥删除后缀后 `/tmp/x` 6 字节。
    let _ = l.register(cell(
        5,
        OP_STRIP,
        match readlink_exe(b"/tmp/x (deleted)") {
            Ok(e) => Ok(e.path.len() as u32),
            Err(c) => Err(c),
        },
    ));

    // 6: 两种写法同一（1 = 真）。
    let _ = l.register(cell(6, OP_IDENT, Ok(identity(b"/usr//bin/./sh", b"/usr/bin/sh") as u32)));

    // 7: `/a:/b:/c` 三段候选。
    let _ = l.register(cell(7, OP_SEARCH, Ok(candidates(b"busybox", b"/a:/b:/c") as u32)));

    // 8: 空路径。
    let _ = l.register(cell(
        8,
        OP_NORM,
        match normalize(b"") {
            Ok(_) => Ok(0),
            Err(c) => Err(c),
        },
    ));

    // 9: 相对路径。
    let _ = l.register(cell(
        9,
        OP_NORM,
        match normalize(b"bin/busybox") {
            Ok(_) => Ok(0),
            Err(c) => Err(c),
        },
    ));

    // 10: `..` 越根。
    let _ = l.register(cell(
        10,
        OP_NORM,
        match normalize(b"/../../etc/passwd") {
            Ok(_) => Ok(0),
            Err(c) => Err(c),
        },
    ));

    // 11: 超长（100 字节）。
    let _ = l.register(cell(
        11,
        OP_NORM,
        {
            let buf = padded(PATH_CAP + 4);
            match buf.get(..PATH_CAP + 4) {
                Some(raw) => match normalize(raw) {
                    Ok(_) => Ok(0),
                    Err(c) => Err(c),
                },
                None => Err(SX_ELEN),
            }
        },
    ));

    // 12: 单段 33 字节。
    let _ = l.register(cell(
        12,
        OP_NORM,
        {
            let buf = padded(SEG_CAP + 1);
            match buf.get(..SEG_CAP + 2) {
                Some(raw) => match normalize(raw) {
                    Ok(_) => Ok(0),
                    Err(c) => Err(c),
                },
                None => Err(SX_ESEG),
            }
        },
    ));

    // 13: 无链接、无 argv[0]。
    let _ = l.register(cell(
        13,
        OP_FALLBACK,
        match resolve_exe(b"", b"", b"/bin") {
            Ok(_) => Ok(0),
            Err(c) => Err(c),
        },
    ));

    l
}

// ---------------------------------------------------------------------------
// 判据面
// ---------------------------------------------------------------------------

/// 归一：折叠 / 点段 / 尾斜杠 / 根。
fn norm_ok() -> bool {
    let a = normalize(b"/usr//bin/./busybox") == Ok(mk(b"/usr/bin/busybox"));
    let b = normalize(b"/usr/bin/") == Ok(mk(b"/usr/bin"));
    let c = normalize(b"/") == Ok(mk(b"/"));
    let d = normalize(b"//") == Ok(mk(b"/"));
    let e = normalize(b"/./") == Ok(mk(b"/"));
    let f = normalize(b"/a/./b/./c/") == Ok(mk(b"/a/b/c"));
    let g = match normalize(b"/a/b") {
        Ok(p) => p.depth() == 2 && p.basename() == b"b" && p.is_absolute() && !p.is_root(),
        Err(_) => false,
    };
    a && b && c && d && e && f && g
}

/// 点段：正常退段与越根拒绝。
fn dot_ok() -> bool {
    let a = normalize(b"/a/../b") == Ok(mk(b"/b"));
    let b = normalize(b"/a/b/..") == Ok(mk(b"/a"));
    let c = normalize(b"/a/b/../..") == Ok(mk(b"/"));
    let d = normalize(b"/..") == Err(SX_EDOT);
    let e = normalize(b"/a/../../b") == Err(SX_EDOT);
    let f = normalize(b"/a//../..") == Err(SX_EDOT);
    let g = normalize(b"/a/../../..") == Err(SX_EDOT);
    a && b && c && d && e && f && g
}

/// 段长 / 段数 / 字节 / 空 / 相对 五类闸。
fn seg_ok() -> bool {
    let a = normalize(b"").err() == Some(SX_EMPTY);
    let b = normalize(b"bin/ls").err() == Some(SX_EREL);
    let c = normalize(b"/a\0b").err() == Some(SX_EBYTE);
    let d = normalize(&[b'/'; 4]).is_ok();
    let buf = padded(SEG_CAP + 1);
    let e = match buf.get(..SEG_CAP + 2) {
        Some(raw) => normalize(raw).err() == Some(SX_ESEG),
        None => false,
    };
    let long = padded(PATH_CAP + 4);
    let f = match long.get(..PATH_CAP + 4) {
        Some(raw) => normalize(raw).err() == Some(SX_ELEN),
        None => false,
    };
    let g = normalize(b"/a/b/c/d/e/f/g/h/i/j/k").err() == Some(SX_EDEEP);
    a && b && c && d && e && f && g
}

/// 基名与目录。
fn base_ok() -> bool {
    let a = mk(b"/usr/local/bin/busybox");
    let b = a.basename() == b"busybox";
    let c = a.dirname() == mk(b"/usr/local/bin");
    let d = mk(b"/").dirname() == mk(b"/");
    let e = mk(b"/").basename() == b"/";
    let f = mk(b"/a").dirname() == mk(b"/");
    let g = mk(b"/a").basename() == b"a";
    let h = mk(b"/a/b").dirname().dirname() == mk(b"/");
    a.is_absolute() && !a.is_root() && b && c && d && e && f && g && h
}

/// 自链接读取：来源 / 删除位 / 空目标。
fn readlink_ok() -> bool {
    let a = match readlink_exe(b"/usr/bin/busybox") {
        Ok(e) => e.source == Source::Link && !e.is_deleted() && e.path == mk(b"/usr/bin/busybox"),
        Err(_) => return false,
    };
    let b = match readlink_exe(b"/tmp/x (deleted)") {
        Ok(e) => e.is_deleted() && e.path == mk(b"/tmp/x") && e.path.len() == 6,
        Err(_) => return false,
    };
    let c = readlink_exe(b"") == Err(SX_ENOENT);
    let d = readlink_exe(b" (deleted)").err() == Some(SX_EMPTY);
    let e = readlink_exe(b"bin/busybox").err() == Some(SX_EREL);
    let f = match readlink_exe(b"/bin//./busybox") {
        Ok(x) => x.path == mk(b"/bin/busybox"),
        Err(_) => false,
    };
    a && b && c && d && e && f
}

/// argv[0] 回退。
fn fallback_ok() -> bool {
    let a = match resolve_exe(b"", b"/bin/busybox", b"") {
        Ok(e) => e.source == Source::Argv0 && e.path == mk(b"/bin/busybox"),
        Err(_) => false,
    };
    let b = match resolve_exe(b"", b"busybox", DEFAULT_PATH) {
        Ok(e) => e.source == Source::PathSearch && e.path == mk(b"/usr/local/bin/busybox"),
        Err(_) => false,
    };
    let c = resolve_exe(b"", b"", b"/bin") == Err(SX_ENOENT);
    let d = resolve_exe(b"", b"nosuch", b"/bin:/usr/bin") == Err(SX_ENOENT);
    let e = match resolve_exe(b"", b"busybox", b":/bin:") {
        Ok(x) => x.path == mk(b"/bin/busybox"),
        Err(_) => false,
    };
    let f = match resolve_exe(b"/usr/bin/busybox", b"/bin/ls", b"") {
        Ok(x) => x.path == mk(b"/usr/bin/busybox"),
        Err(_) => false,
    };
    a && b && c && d && e && f
}

/// PATH 候选与检索顺序。
fn search_ok() -> bool {
    let a = candidates(b"busybox", b"/a:/b:/c") == 3;
    let b = candidates(b"busybox", b"") == 0;
    let c = candidates(b"busybox", b":/bin:") == 1;
    let d = candidates(b"busybox", b"/bin") == 1;
    let e = search_path(b"busybox", b"/bin") == Ok(mk(b"/bin/busybox"));
    let f = search_path(b"/bin/ls", b"") == Ok(mk(b"/bin/ls"));
    let g = search_path(b"init", b"/usr/bin:/bin") == Err(SX_ENOENT);
    let h = search_path(b"", b"/bin") == Err(SX_ENOENT);
    a && b && c && d && e && f && g && h
}

/// 同一判定。
fn ident_ok() -> bool {
    let a = identity(b"/usr//bin/./sh", b"/usr/bin/sh");
    let b = identity(b"/bin/busybox", b"/usr/bin/busybox");
    let c = identity(b"/bin/", b"/bin");
    let d = !identity(b"/bin/ls", b"/bin/sh");
    let e = !identity(b"", b"/bin/sh");
    let f = !identity(b"bin/sh", b"/bin/sh");
    let g = identity(b"/a/../bin/sh", b"/bin/sh");
    a && !b && c && d && e && f && g
}

/// 删除后缀。
fn strip_ok() -> bool {
    let (a, da) = split_deleted(b"/tmp/x (deleted)");
    let (b, db) = split_deleted(b"/tmp/x");
    let (c, dc) = split_deleted(b" (deleted)");
    let (d, dd) = split_deleted(b"/tmp/x (deleted) ");
    eq_bytes(a, b"/tmp/x") && da && eq_bytes(b, b"/tmp/x") && !db && c.is_empty() && dc
        && eq_bytes(d, b"/tmp/x (deleted) ") && !dd
}

/// 前缀归属。
fn prefix_ok() -> bool {
    let p = mk(b"/usr/bin/busybox");
    let a = p.is_under(&mk(b"/usr/bin"));
    let b = p.is_under(&mk(b"/usr"));
    let c = p.is_under(&mk(b"/"));
    let d = p.is_under(&p);
    let e = !p.is_under(&mk(b"/usr/binge"));
    let f = !mk(b"/usr").is_under(&p);
    let g = !p.is_under(&mk(b"/bin"));
    a && b && c && d && e && f && g
}

/// 指纹。
fn seal_ok() -> bool {
    let a = mk(b"/usr/bin/busybox");
    let b = mk(b"/usr/bin/busybox");
    let c = mk(b"/usr/bin/sh");
    if a.seal() != b.seal() || a.seal() == 0 {
        return false;
    }
    let e1 = match readlink_exe(b"/tmp/x (deleted)") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let e2 = match readlink_exe(b"/tmp/x (deleted)") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let e3 = match readlink_exe(b"/tmp/x") {
        Ok(v) => v,
        Err(_) => return false,
    };
    a.seal() != c.seal() && e1.seal() == e2.seal() && e1.seal() != e3.seal()
}

/// 便捷构造：假定已归一。
fn mk(raw: &[u8]) -> Path {
    match normalize(raw) {
        Ok(v) => v,
        Err(_) => Path::empty(),
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
    if l.seal() != build_ledger().seal() {
        return false;
    }
    let mut l2 = build_ledger();
    let root = match l2.freeze() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if l2.replay() != Ok(root) || !l2.is_frozen() {
        return false;
    }
    if l2.freeze() != Err(SX_EBAL) {
        return false;
    }
    l2.rejects() == 1
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    if let Some(dst) = out.get_mut(0) {
        *dst = norm_ok() && dot_ok() && seg_ok();
    }
    if let Some(dst) = out.get_mut(1) {
        *dst = base_ok() && readlink_ok() && fallback_ok();
    }
    if let Some(dst) = out.get_mut(2) {
        *dst = search_ok() && ident_ok() && strip_ok();
    }
    if let Some(dst) = out.get_mut(3) {
        *dst = prefix_ok() && seal_ok() && ledger_ok();
    }
    out
}

/// 判据集合（13 项）。
pub fn run_unx11594_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11594.exe");

    s.add("SX01-路径归一", norm_ok(), "static");
    s.add("SX02-点段处理", dot_ok(), "static");
    s.add("SX03-段长与容量", seg_ok(), "static");
    s.add("SX04-基名与目录", base_ok(), "static");
    s.add("SX05-自链接读取", readlink_ok(), "static");
    s.add("SX06-argv0 回退", fallback_ok(), "static");
    s.add("SX07-PATH 检索", search_ok(), "static");
    s.add("SX08-路径同一", ident_ok(), "static");
    s.add("SX09-删除后缀", strip_ok(), "static");
    s.add("SX10-前缀归属", prefix_ok(), "static");
    s.add("SX11-路径指纹", seal_ok(), "static");
    s.add("SX12-账本对账", build_ledger().audit() == Ok(()), "static");
    let four = inject_four();
    s.add("SX13-冻结重放四注入", four[0] && four[1] && four[2] && four[3], "static");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_cases() {
        assert_eq!(normalize(b"/usr//bin/./busybox"), Ok(mk(b"/usr/bin/busybox")));
        assert_eq!(normalize(b"/usr/bin/"), Ok(mk(b"/usr/bin")));
        assert_eq!(normalize(b"//"), Ok(mk(b"/")));
        assert_eq!(normalize(b"/a/./b/./c/"), Ok(mk(b"/a/b/c")));
    }

    #[test]
    fn dotdot_cases() {
        assert_eq!(normalize(b"/a/../b"), Ok(mk(b"/b")));
        assert_eq!(normalize(b"/a/b/.."), Ok(mk(b"/a")));
        assert_eq!(normalize(b"/a/b/../.."), Ok(mk(b"/")));
        assert_eq!(normalize(b"/.."), Err(SX_EDOT));
        assert_eq!(normalize(b"/a/../../b"), Err(SX_EDOT));
    }

    #[test]
    fn gates() {
        assert_eq!(normalize(b""), Err(SX_EMPTY));
        assert_eq!(normalize(b"bin/ls"), Err(SX_EREL));
        assert_eq!(normalize(b"/a\0b"), Err(SX_EBYTE));
        assert_eq!(normalize(b"/a/b/c/d/e/f/g/h/i/j/k"), Err(SX_EDEEP));
    }

    #[test]
    fn segment_limits() {
        assert_eq!(normalize(b"/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").err(), Some(SX_ESEG));
        assert!(normalize(b"/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").is_ok());
        let buf = padded(PATH_CAP + 4);
        assert_eq!(
            buf.get(..PATH_CAP + 4).map(|r| normalize(r).err()),
            Some(Some(SX_ELEN))
        );
    }

    #[test]
    fn basename_dirname() {
        let p = mk(b"/usr/local/bin/busybox");
        assert_eq!(p.basename(), b"busybox");
        assert_eq!(p.dirname(), mk(b"/usr/local/bin"));
        assert_eq!(mk(b"/").dirname(), mk(b"/"));
        assert_eq!(mk(b"/a").dirname(), mk(b"/"));
        assert_eq!(p.depth(), 4);
    }

    #[test]
    fn readlink_source_and_deleted() {
        let e = readlink_exe(b"/usr/bin/busybox").expect("e");
        assert_eq!(e.source, Source::Link);
        assert!(!e.is_deleted());
        assert_eq!(e.path, mk(b"/usr/bin/busybox"));
        assert_eq!(e.text().as_str(), "/usr/bin/busybox [link]");
        let d = readlink_exe(b"/tmp/x (deleted)").expect("d");
        assert!(d.is_deleted());
        assert_eq!(d.text().as_str(), "/tmp/x [link,deleted]");
        assert_eq!(d.path.len(), 6);
        assert_eq!(readlink_exe(b"").err(), Some(SX_ENOENT));
    }

    #[test]
    fn fallback_sources() {
        let a = resolve_exe(b"", b"/bin/busybox", b"").expect("a");
        assert_eq!(a.source, Source::Argv0);
        let b = resolve_exe(b"", b"busybox", DEFAULT_PATH).expect("b");
        assert_eq!(b.source, Source::PathSearch);
        assert_eq!(b.path, mk(b"/usr/local/bin/busybox"));
        assert_eq!(resolve_exe(b"", b"", b"/bin").err(), Some(SX_ENOENT));
    }

    #[test]
    fn path_search_order() {
        assert_eq!(search_path(b"busybox", b"/bin"), Ok(mk(b"/bin/busybox")));
        assert_eq!(
            search_path(b"busybox", b"/usr/bin:/bin"),
            Ok(mk(b"/usr/bin/busybox"))
        );
        assert_eq!(search_path(b"init", b"/usr/bin:/bin"), Err(SX_ENOENT));
        assert_eq!(candidates(b"x", b"/a:/b:/c"), 3);
    }

    #[test]
    fn identity_forms() {
        assert!(identity(b"/usr//bin/./sh", b"/usr/bin/sh"));
        assert!(identity(b"/a/../bin/sh", b"/bin/sh"));
        assert!(!identity(b"/bin/busybox", b"/usr/bin/busybox"));
        assert!(!identity(b"", b"/bin/sh"));
    }

    #[test]
    fn deleted_split() {
        let (a, d) = split_deleted(b"/tmp/x (deleted)");
        assert_eq!(a, b"/tmp/x");
        assert!(d);
        let (b, d2) = split_deleted(b"/tmp/x");
        assert_eq!(b, b"/tmp/x");
        assert!(!d2);
    }

    #[test]
    fn prefix_membership() {
        let p = mk(b"/usr/bin/busybox");
        assert!(p.is_under(&mk(b"/usr/bin")));
        assert!(p.is_under(&mk(b"/")));
        assert!(!p.is_under(&mk(b"/usr/binge")));
        assert!(!mk(b"/usr").is_under(&p));
    }

    #[test]
    fn seal_stability() {
        assert_eq!(mk(b"/bin/ls").seal(), mk(b"/bin/ls").seal());
        assert!(mk(b"/bin/ls").seal() != mk(b"/bin/sh").seal());
        let e = readlink_exe(b"/tmp/x (deleted)").expect("e");
        assert!(e.seal() != readlink_exe(b"/tmp/x").expect("f").seal());
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.sample(0).map(|s| s.value), Some(16));
        assert_eq!(l.sample(5).map(|s| s.value), Some(6));
        assert_eq!(l.sample(10).map(|s| s.code), Some(SX_EDOT));
        assert_eq!(l.sample(13).map(|s| s.code), Some(SX_ENOENT));
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = l.freeze().expect("freeze");
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(SX_EBAL));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn injections() {
        assert_eq!(inject_four(), [true, true, true, true]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11594_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
