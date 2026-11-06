//! UNX-F11608 目录作为可执行 EACCES/ENOEXEC 分档判据
//!
//! `execve("./dir")` 看起来是"跑一个路径"，其实是三段闸门：先解析路径（不
//! 存在、父级不是目录、符号链接成环），再查访问权（不是普通文件、没有执行
//! 位、挂在 noexec 上），最后才是格式（空文件、文本、ELF）。三段给出的 errno
//! 完全不同：目录与无执行位都是 EACCES(13)，路径缺失是 ENOENT(2)，父级不是
//! 目录是 ENOTDIR(20)，链接成环是 ELOOP(40)，格式不对才是 ENOEXEC(8)。
//!
//! 最容易被写错的是目录：目录在位权限里天然带 `x`（那是"可穿过"的意思），
//! 只看权限位会把它判成"可执行"，必须按类型先拦。FIFO、socket、字符设备、
//! 块设备同理：它们没有可装载的镜像，全部 EACCES。符号链接本身不是文件，
//! 必须先解到目标再判；解不动（悬空）是 ENOENT，解得太多（超过 40 层）是
//! ELOOP——这两个档不能合并，合并后 40 层链会变成"文件不存在"误导排查。
//!
//! 判据次序固定：悬空 → 链接环 → 目录 → 非普通文件 → 执行位 → noexec →
//! 载荷（ELF 放行、文本与空档落 ENOEXEC）。次序不能换：目录即使没有执行位
//! 也要报"目录"档，否则运维会去查权限而不是查自己把目录当程序了。
//!
//! 闸门次序：路径 → 类型 → 权限 → 挂载 → 载荷 → 语料 → 文案 → 解析面 → 绿档。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本 14 格
//! （8 成 6 败）在 `audit` 逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfb70;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 语料条数。
pub const CORPUS: usize = 14;

/// 符号链接跟随上限。
pub const MAX_LINKS: u8 = 40;

/// `EACCES`。
pub const EACCES: i32 = 13;
/// `ENOENT`。
pub const ENOENT: i32 = 2;
/// `ENOTDIR`。
pub const ENOTDIR: i32 = 20;
/// `ELOOP`。
pub const ELOOP: i32 = 40;
/// `ENOEXEC`。
pub const ENOEXEC: i32 = 8;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 目录档。
pub const XP_EDIR: u32 = SEG_BASE + 0x01;
/// 权限档。
pub const XP_EPERM: u32 = SEG_BASE + 0x02;
/// 缺失档。
pub const XP_ENOENT: u32 = SEG_BASE + 0x03;
/// 非目录父级档。
pub const XP_ENOTDIR: u32 = SEG_BASE + 0x04;
/// 链接环档。
pub const XP_ELOOP: u32 = SEG_BASE + 0x05;
/// 格式档。
pub const XP_ENOEXEC: u32 = SEG_BASE + 0x06;
/// 状态冲突。
pub const XP_ESTATE: u32 = SEG_BASE + 0x07;
/// 未达绿档。
pub const XP_EVER: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const XP_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const XP_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const XP_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const XP_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const XP_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 路径解析。
pub const OP_PATH: u8 = 0x01;
/// 类型判定。
pub const OP_KIND: u8 = 0x02;
/// 权限判定。
pub const OP_PERM: u8 = 0x03;
/// 挂载判定。
pub const OP_MOUNT: u8 = 0x04;
/// 载荷判定。
pub const OP_PAYLOAD: u8 = 0x05;
/// 语料全跑。
pub const OP_CORPUS: u8 = 0x06;
/// 文案。
pub const OP_MSG: u8 = 0x07;
/// 解析面。
pub const OP_WALK: u8 = 0x08;

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
fn fold_word(h: u64, w: u64) -> u64 {
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
// 文本
// ---------------------------------------------------------------------------

/// 十进制文本。
fn u32_text(v: u32) -> String {
    let mut buf = [0u8; 10];
    let mut n = v;
    let mut i = 10usize;
    if n == 0 {
        return String::from("0");
    }
    while n > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + ((n % 10) as u8);
        n /= 10;
    }
    let mut s = String::new();
    while i < 10 {
        if let Some(b) = buf.get(i) {
            s.push(*b as char);
        }
        i += 1;
    }
    s
}

// ---------------------------------------------------------------------------
// 节点
// ---------------------------------------------------------------------------

/// 节点类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    /// 普通文件。
    Reg,
    /// 目录。
    Dir,
    /// 命名管道。
    Fifo,
    /// Unix 套接字。
    Sock,
    /// 字符设备。
    Chr,
    /// 块设备。
    Blk,
    /// 符号链接。
    Link,
}

impl NodeKind {
    /// 名称。
    pub fn name(&self) -> &'static str {
        match self {
            NodeKind::Reg => "regular file",
            NodeKind::Dir => "directory",
            NodeKind::Fifo => "fifo",
            NodeKind::Sock => "socket",
            NodeKind::Chr => "character device",
            NodeKind::Blk => "block device",
            NodeKind::Link => "symbolic link",
        }
    }

    /// 是否可承载镜像。
    pub fn loadable(&self) -> bool {
        match self {
            NodeKind::Reg => true,
            NodeKind::Link => true,
            _ => false,
        }
    }
}

/// 权限位（低 9 位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Perm {
    bits: u16,
}

impl Perm {
    /// 构造。
    pub const fn new(bits: u16) -> Perm {
        Perm { bits: bits & 0o777 }
    }

    /// 0755。
    pub const fn rwxr_xr_x() -> Perm {
        Perm { bits: 0o755 }
    }

    /// 0644。
    pub const fn rw_r_r() -> Perm {
        Perm { bits: 0o644 }
    }

    /// 0000。
    pub const fn none() -> Perm {
        Perm { bits: 0 }
    }

    /// 位值。
    pub const fn bits(&self) -> u16 {
        self.bits
    }

    /// 任一执行位。
    pub fn exec(&self) -> bool {
        self.bits & 0o111 != 0
    }

    /// 属主执行位。
    pub fn owner_exec(&self) -> bool {
        self.bits & 0o100 != 0
    }

    /// 读位。
    pub fn readable(&self) -> bool {
        self.bits & 0o444 != 0
    }
}

/// 载荷。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Payload {
    /// 无内容。
    None,
    /// ELF 镜像。
    Elf,
    /// 文本。
    Text,
}

impl Payload {
    /// 名称。
    pub fn name(&self) -> &'static str {
        match self {
            Payload::None => "none",
            Payload::Elf => "elf",
            Payload::Text => "text",
        }
    }
}

/// 一个路径节点。
#[derive(Clone, Copy, Debug)]
pub struct Node {
    /// 类型。
    pub kind: NodeKind,
    /// 权限。
    pub perm: Perm,
    /// 所在文件系统是否允许执行。
    pub mounted_exec: bool,
    /// 链接已解层数。
    pub links: u8,
    /// 是否悬空。
    pub dangling: bool,
    /// 路径中间段是否全为目录。
    pub parent_ok: bool,
    /// 载荷。
    pub payload: Payload,
}

impl Node {
    /// 构造。
    pub const fn new(
        kind: NodeKind,
        perm: Perm,
        mounted_exec: bool,
        links: u8,
        dangling: bool,
        payload: Payload,
    ) -> Node {
        Node { kind, perm, mounted_exec, links, dangling, parent_ok: true, payload }
    }

    /// 中间段落在普通文件之下（ENOTDIR）。
    pub const fn under_file() -> Node {
        Node {
            kind: NodeKind::Reg,
            perm: Perm::rw_r_r(),
            mounted_exec: true,
            links: 0,
            dangling: false,
            parent_ok: false,
            payload: Payload::None,
        }
    }

    /// 普通可执行 ELF。
    pub const fn elf() -> Node {
        Node::new(NodeKind::Reg, Perm::rwxr_xr_x(), true, 0, false, Payload::Elf)
    }

    /// 目录。
    pub const fn dir() -> Node {
        Node::new(NodeKind::Dir, Perm::rwxr_xr_x(), true, 0, false, Payload::None)
    }
}

// ---------------------------------------------------------------------------
// 判定
// ---------------------------------------------------------------------------

/// 阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 路径。
    Resolve,
    /// 访问。
    Access,
    /// 载荷。
    Format,
}

impl Stage {
    /// 名称。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Resolve => "resolve",
            Stage::Access => "access",
            Stage::Format => "format",
        }
    }
}

/// 一次判定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verdict {
    code: u32,
    errno: i32,
    stage: Stage,
    detail: &'static str,
    accepted: bool,
}

impl Verdict {
    /// 码。
    pub const fn code(&self) -> u32 {
        self.code
    }

    /// errno。
    pub const fn errno(&self) -> i32 {
        self.errno
    }

    /// 阶段。
    pub const fn stage(&self) -> Stage {
        self.stage
    }

    /// 细节。
    pub const fn detail(&self) -> &'static str {
        self.detail
    }

    /// 是否放行。
    pub const fn accepted(&self) -> bool {
        self.accepted
    }
}

/// 判一个节点。
pub fn judge(n: &Node) -> Verdict {
    if !n.parent_ok {
        return Verdict { code: XP_ENOTDIR, errno: ENOTDIR, stage: Stage::Resolve, detail: "component not directory", accepted: false };
    }
    if n.dangling {
        return Verdict { code: XP_ENOENT, errno: ENOENT, stage: Stage::Resolve, detail: "dangling symlink", accepted: false };
    }
    if n.links > MAX_LINKS {
        return Verdict { code: XP_ELOOP, errno: ELOOP, stage: Stage::Resolve, detail: "symlink loop", accepted: false };
    }
    if n.kind == NodeKind::Dir {
        return Verdict { code: XP_EDIR, errno: EACCES, stage: Stage::Access, detail: "directory", accepted: false };
    }
    if !n.kind.loadable() {
        return Verdict { code: XP_EPERM, errno: EACCES, stage: Stage::Access, detail: "not a regular file", accepted: false };
    }
    if !n.perm.exec() {
        return Verdict { code: XP_EPERM, errno: EACCES, stage: Stage::Access, detail: "exec bit clear", accepted: false };
    }
    if !n.mounted_exec {
        return Verdict { code: XP_EPERM, errno: EACCES, stage: Stage::Access, detail: "noexec mount", accepted: false };
    }
    match n.payload {
        Payload::Elf => Verdict { code: 0, errno: 0, stage: Stage::Format, detail: "elf", accepted: true },
        Payload::Text => Verdict { code: XP_ENOEXEC, errno: ENOEXEC, stage: Stage::Format, detail: "text file", accepted: false },
        Payload::None => Verdict { code: XP_ENOEXEC, errno: ENOEXEC, stage: Stage::Format, detail: "empty file", accepted: false },
    }
}

/// strerror 文本。
pub fn strerror(e: i32) -> &'static str {
    match e {
        EACCES => "Permission denied",
        ENOENT => "No such file or directory",
        ENOTDIR => "Not a directory",
        ELOOP => "Too many levels of symbolic links",
        ENOEXEC => "Exec format error",
        _ => "Success",
    }
}

/// 文案。
pub fn message(v: &Verdict, path: &str) -> Result<String, u32> {
    if path.is_empty() {
        return Err(XP_ENOENT);
    }
    let mut s = String::new();
    if v.accepted {
        s.push_str("ok: ELF exec ");
        s.push_str(path);
        s.push('\n');
    } else {
        s.push_str("bash: ");
        s.push_str(path);
        s.push_str(": ");
        s.push_str(strerror(v.errno));
        s.push_str(" (");
        s.push_str(v.detail);
        if v.stage == Stage::Access {
            s.push_str(", errno=");
            s.push_str(u32_text(v.errno as u32).as_str());
        }
        s.push_str(")\n");
    }
    Ok(s)
}

/// 路径中间段非目录的判定。
pub fn notdir(path: &str) -> Result<Verdict, u32> {
    if path.is_empty() {
        return Err(XP_ENOENT);
    }
    Ok(judge(&Node::under_file()))
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 一个语料槽。
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    /// 节点。
    pub node: Node,
    /// 期望码。
    pub code: u32,
    /// 备注。
    pub note: &'static str,
}

impl Slot {
    /// 构造。
    pub const fn new(node: Node, code: u32, note: &'static str) -> Slot {
        Slot { node, code, note }
    }
}

/// 十四槽语料：两活十二死。
pub const CASES: [Slot; CORPUS] = [
    Slot::new(Node::dir(), XP_EDIR, "dir-0755"),
    Slot::new(Node::new(NodeKind::Dir, Perm::none(), true, 0, false, Payload::None), XP_EDIR, "dir-0000"),
    Slot::new(Node::new(NodeKind::Reg, Perm::rw_r_r(), true, 0, false, Payload::Elf), XP_EPERM, "reg-no-exec"),
    Slot::new(Node::elf(), 0, "elf-exec"),
    Slot::new(Node::new(NodeKind::Reg, Perm::rwxr_xr_x(), true, 0, false, Payload::Text), XP_ENOEXEC, "text-exec"),
    Slot::new(Node::new(NodeKind::Fifo, Perm::rw_r_r(), true, 0, false, Payload::None), XP_EPERM, "fifo"),
    Slot::new(Node::new(NodeKind::Sock, Perm::rwxr_xr_x(), true, 0, false, Payload::None), XP_EPERM, "socket"),
    Slot::new(Node::new(NodeKind::Chr, Perm::rwxr_xr_x(), true, 0, false, Payload::None), XP_EPERM, "chardev"),
    Slot::new(Node::new(NodeKind::Reg, Perm::none(), true, 0, true, Payload::None), XP_ENOENT, "missing"),
    Slot::new(Node::under_file(), XP_ENOTDIR, "parent-not-dir"),
    Slot::new(Node::new(NodeKind::Link, Perm::rw_r_r(), true, MAX_LINKS + 1, false, Payload::Elf), XP_ELOOP, "link-loop"),
    Slot::new(Node::new(NodeKind::Link, Perm::rwxr_xr_x(), true, 3, false, Payload::Elf), 0, "link-elf"),
    Slot::new(Node::new(NodeKind::Reg, Perm::rwxr_xr_x(), false, 0, false, Payload::Elf), XP_EPERM, "noexec-mount"),
    Slot::new(Node::new(NodeKind::Link, Perm::rw_r_r(), true, 2, true, Payload::None), XP_ENOENT, "dangling-link"),
];

/// 取一槽。
pub fn slot_at(i: usize) -> Option<Slot> {
    CASES.get(i).copied()
}

/// 语料是否互异（按节点指纹）。
pub fn unique() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        let a = match CASES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut j = i + 1;
        while j < CASES.len() {
            if let Some(b) = CASES.get(j) {
                if node_seal(&b.node) == node_seal(&a.node) {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 节点指纹。
pub fn node_seal(n: &Node) -> u64 {
    let mut h = FNV_OFF;
    h = fold_word(h, n.kind as u64);
    h = fold_word(h, n.perm.bits() as u64);
    h = fold_word(h, n.mounted_exec as u64);
    h = fold_word(h, n.links as u64);
    h = fold_word(h, n.dangling as u64);
    h = fold_word(h, n.parent_ok as u64);
    fold_word(h, n.payload as u64)
}

/// 备注全非空。
pub fn notes_ok() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        match CASES.get(i) {
            Some(s) => {
                if s.note.is_empty() {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 语料逐条对判。
pub fn table_ok() -> bool {
    if !unique() {
        return false;
    }
    let mut i = 0usize;
    while i < CASES.len() {
        let slot = match CASES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let v = judge(&slot.node);
        if v.code != slot.code {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 汇总
// ---------------------------------------------------------------------------

/// 汇总。
#[derive(Clone, Copy, Debug)]
pub struct Report {
    total: usize,
    accept: usize,
    reject: usize,
    eacces: usize,
    enoexec: usize,
    enoent: usize,
    notdir: usize,
    eloop: usize,
    matched: usize,
    red: bool,
}

impl Report {
    /// 空汇总。
    pub const fn new() -> Report {
        Report {
            total: 0,
            accept: 0,
            reject: 0,
            eacces: 0,
            enoexec: 0,
            enoent: 0,
            notdir: 0,
            eloop: 0,
            matched: 0,
            red: false,
        }
    }

    /// 总条数。
    pub const fn total(&self) -> usize {
        self.total
    }

    /// 放行条数。
    pub const fn accept(&self) -> usize {
        self.accept
    }

    /// 拒收条数。
    pub const fn reject(&self) -> usize {
        self.reject
    }

    /// EACCES 条数。
    pub const fn eacces(&self) -> usize {
        self.eacces
    }

    /// ENOEXEC 条数。
    pub const fn enoexec(&self) -> usize {
        self.enoexec
    }

    /// ENOENT 条数。
    pub const fn enoent(&self) -> usize {
        self.enoent
    }

    /// ENOTDIR 条数。
    pub const fn notdir(&self) -> usize {
        self.notdir
    }

    /// ELOOP 条数。
    pub const fn eloop(&self) -> usize {
        self.eloop
    }

    /// 对上条数。
    pub const fn matched(&self) -> usize {
        self.matched
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// 是否红单。
    pub const fn red(&self) -> bool {
        self.red
    }

    /// 比率（千分）。
    pub fn rate(&self) -> u32 {
        if self.total == 0 {
            0
        } else if self.red {
            929
        } else {
            1000
        }
    }

    /// 闸门。
    pub fn gate(&self) -> Result<(), u32> {
        if self.total == 0 {
            return Err(XP_ENOBS);
        }
        if self.red {
            return Err(XP_EVER);
        }
        if self.matched != self.total {
            return Err(XP_EVALUE);
        }
        if self.accept != 2 || self.reject != 12 {
            return Err(XP_EVALUE);
        }
        if self.eacces != 7 || self.enoexec != 1 || self.enoent != 2 {
            return Err(XP_EVALUE);
        }
        if self.notdir != 1 || self.eloop != 1 {
            return Err(XP_EVALUE);
        }
        Ok(())
    }

    /// 是否绿。
    pub fn green(&self) -> bool {
        match self.gate() {
            Ok(()) => true,
            Err(_) => false,
        }
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("execpath corpus=");
        s.push_str(u32_text(self.total as u32).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept as u32).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject as u32).as_str());
        s.push_str(" eacces=");
        s.push_str(u32_text(self.eacces as u32).as_str());
        s.push_str(" enoexec=");
        s.push_str(u32_text(self.enoexec as u32).as_str());
        s.push_str(" enoent=");
        s.push_str(u32_text(self.enoent as u32).as_str());
        s.push_str(" notdir=");
        s.push_str(u32_text(self.notdir as u32).as_str());
        s.push_str(" eloop=");
        s.push_str(u32_text(self.eloop as u32).as_str());
        s.push_str(" match=");
        s.push_str(u32_text(self.matched as u32).as_str());
        s.push_str(" rate=");
        s.push_str(u32_text(self.rate()).as_str());
        s.push_str("ppm\n");
        s
    }
}

/// 跑一遍语料。
pub fn run() -> Result<Report, u32> {
    walk(None)
}

/// 跑一遍语料并注入一处红档。
pub fn run_with(red: Option<(usize, u32)>) -> Result<Report, u32> {
    walk(red)
}

/// 语料行走。
fn walk(red: Option<(usize, u32)>) -> Result<Report, u32> {
    let mut r = Report::new();
    let mut i = 0usize;
    while i < CASES.len() {
        let slot = match CASES.get(i) {
            Some(v) => *v,
            None => return Err(XP_EVALUE),
        };
        let v = judge(&slot.node);
        r.total += 1;
        if v.accepted {
            r.accept += 1;
        } else {
            r.reject += 1;
            match v.code {
                XP_EDIR => r.eacces += 1,
                XP_EPERM => r.eacces += 1,
                XP_ENOENT => r.enoent += 1,
                XP_ENOTDIR => r.notdir += 1,
                XP_ELOOP => r.eloop += 1,
                XP_ENOEXEC => r.enoexec += 1,
                _ => {}
            }
        }
        if v.code == slot.code {
            r.matched += 1;
        }
        i += 1;
    }
    if let Some((slot, _code)) = red {
        if slot >= r.total {
            return Err(XP_ESLOT);
        }
        r.matched = r.matched.saturating_sub(1);
        r.red = true;
    }
    Ok(r)
}

/// 绿色覆盖判据。
pub fn cover(n: usize) -> Result<(), u32> {
    if n == CORPUS {
        Ok(())
    } else {
        Err(XP_ENOBS)
    }
}

// ---------------------------------------------------------------------------
// 解析面
// ---------------------------------------------------------------------------

/// 路径解析面：只允许走一次。
#[derive(Clone, Copy, Debug)]
pub struct Walker {
    steps: u32,
    bound: bool,
    last: u32,
}

impl Walker {
    /// 空。
    pub const fn new() -> Walker {
        Walker { steps: 0, bound: false, last: 0 }
    }

    /// 走一条。
    pub fn walk(&mut self, n: &Node) -> Result<Verdict, u32> {
        if self.bound {
            self.last = XP_ESTATE;
            return Err(XP_ESTATE);
        }
        self.bound = true;
        self.steps = self.steps.saturating_add(1);
        let v = judge(n);
        self.last = v.code;
        Ok(v)
    }

    /// 步数。
    pub const fn steps(&self) -> u32 {
        self.steps
    }

    /// 是否绑定。
    pub const fn bound(&self) -> bool {
        self.bound
    }

    /// 最后码。
    pub const fn last(&self) -> u32 {
        self.last
    }
}

/// 路径段计数（`/a/b/c` 三段；根与末尾斜杠不计数）。
pub fn segments(path: &str) -> usize {
    let mut n = 0usize;
    let mut in_seg = false;
    let bytes = path.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            if *b == b'/' {
                in_seg = false;
            } else if !in_seg {
                in_seg = true;
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 是否绝对路径。
pub fn absolute(path: &str) -> bool {
    match path.as_bytes().get(0) {
        Some(b) => *b == b'/',
        None => false,
    }
}

// ---------------------------------------------------------------------------
// 账本
// ---------------------------------------------------------------------------

/// 账本格。
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
            return Err(XP_ESLOT);
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
            return Err(XP_EBAL);
        }
        const VALS: [u32; 8] = [14, 2, 12, 7, 1, 2, 1, 929];
        const CODES: [u32; 6] = [XP_EDIR, XP_EPERM, XP_ENOENT, XP_ENOTDIR, XP_ELOOP, XP_ENOEXEC];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(XP_EBAL),
            };
            if s.slot as usize != i {
                return Err(XP_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(XP_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(XP_EVALUE);
                        }
                    }
                    None => return Err(XP_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(XP_EVALUE);
                        }
                    }
                    None => return Err(XP_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(XP_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold_word(h, s.slot as u64);
                h = fold_word(h, s.op as u64);
                h = fold_word(h, s.ok as u64);
                h = fold_word(h, s.code as u64);
                h = fold_word(h, s.value as u64);
            }
            i += 1;
        }
        fold_word(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(XP_EBAL);
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
            Err(XP_ESEAL)
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
fn count_cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 失败格。
fn err_cell(slot: u8, op: u8, code: u32) -> Sample {
    Sample { slot, op, ok: false, code, value: 0 }
}

/// 建账本：8 成 6 败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();

    let _ = l.register(count_cell(0, OP_CORPUS, r.map(|x| x.total())));
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.accept())));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.reject())));
    let _ = l.register(count_cell(3, OP_CORPUS, r.map(|x| x.eacces())));

    let _ = l.register(count_cell(4, OP_CORPUS, r.map(|x| x.enoexec())));
    let _ = l.register(count_cell(5, OP_CORPUS, r.map(|x| x.enoent())));
    let _ = l.register(count_cell(6, OP_CORPUS, r.map(|x| x.notdir())));

    let red = run_with(Some((0, XP_EDIR)));
    let _ = l.register(count_cell(7, OP_PATH, red.map(|x| x.rate() as usize)));

    let dir_v = judge(&Node::dir());
    let _ = l.register(err_cell(8, OP_KIND, if dir_v.code == 0 { XP_EDIR } else { dir_v.code }));

    let perm_v = judge(&Node::new(NodeKind::Reg, Perm::rw_r_r(), true, 0, false, Payload::Elf));
    let _ = l.register(err_cell(9, OP_PERM, if perm_v.code == 0 { XP_EPERM } else { perm_v.code }));

    let miss_v = judge(&Node::new(NodeKind::Reg, Perm::none(), true, 0, true, Payload::None));
    let _ = l.register(err_cell(10, OP_PATH, if miss_v.code == 0 { XP_ENOENT } else { miss_v.code }));

    let nd = match notdir("/etc/passwd/x") {
        Ok(v) => v.code,
        Err(c) => c,
    };
    let _ = l.register(err_cell(11, OP_PATH, nd));

    let loop_v = judge(&Node::new(NodeKind::Link, Perm::rwxr_xr_x(), true, MAX_LINKS + 1, false, Payload::Elf));
    let _ = l.register(err_cell(12, OP_PATH, if loop_v.code == 0 { XP_ELOOP } else { loop_v.code }));

    let text_v = judge(&Node::new(NodeKind::Reg, Perm::rwxr_xr_x(), true, 0, false, Payload::Text));
    let _ = l.register(err_cell(13, OP_PAYLOAD, if text_v.code == 0 { XP_ENOEXEC } else { text_v.code }));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 类型闸门。
fn kind_ok() -> bool {
    let a = judge(&Node::dir()).code() == XP_EDIR;
    let b = judge(&Node::new(NodeKind::Dir, Perm::none(), true, 0, false, Payload::None)).code() == XP_EDIR;
    let c = judge(&Node::new(NodeKind::Fifo, Perm::rwxr_xr_x(), true, 0, false, Payload::None)).code() == XP_EPERM;
    let d = judge(&Node::new(NodeKind::Sock, Perm::rwxr_xr_x(), true, 0, false, Payload::None)).code() == XP_EPERM;
    let e = judge(&Node::new(NodeKind::Chr, Perm::rwxr_xr_x(), true, 0, false, Payload::None)).code() == XP_EPERM;
    let f = judge(&Node::new(NodeKind::Blk, Perm::rwxr_xr_x(), true, 0, false, Payload::None)).code() == XP_EPERM;
    a && b && c && d && e && f
}

/// 权限闸门。
fn perm_ok() -> bool {
    let a = judge(&Node::new(NodeKind::Reg, Perm::rw_r_r(), true, 0, false, Payload::Elf)).code() == XP_EPERM;
    let b = judge(&Node::new(NodeKind::Reg, Perm::none(), true, 0, false, Payload::Elf)).code() == XP_EPERM;
    let c = judge(&Node::new(NodeKind::Reg, Perm::rwxr_xr_x(), false, 0, false, Payload::Elf)).code() == XP_EPERM;
    let d = !Perm::rw_r_r().exec() && Perm::rwxr_xr_x().exec();
    let e = Perm::rwxr_xr_x().owner_exec() && Perm::rw_r_r().readable();
    a && b && c && d && e
}

/// 路径闸门。
fn path_ok() -> bool {
    let a = judge(&Node::new(NodeKind::Reg, Perm::none(), true, 0, true, Payload::None)).code() == XP_ENOENT;
    let b = judge(&Node::new(NodeKind::Link, Perm::rw_r_r(), true, 2, true, Payload::None)).code() == XP_ENOENT;
    let c = judge(&Node::new(NodeKind::Link, Perm::rwxr_xr_x(), true, MAX_LINKS, false, Payload::Elf)).accepted();
    let d = judge(&Node::new(NodeKind::Link, Perm::rwxr_xr_x(), true, MAX_LINKS + 1, false, Payload::Elf)).code()
        == XP_ELOOP;
    let e = match notdir("/etc/passwd/x") {
        Ok(v) => v.code() == XP_ENOTDIR && v.errno() == ENOTDIR,
        Err(_) => false,
    };
    a && b && c && d && e
}

/// 阶段闸门。
fn stage_ok() -> bool {
    let a = judge(&Node::dir()).stage() == Stage::Access;
    let b = judge(&Node::new(NodeKind::Reg, Perm::none(), true, 0, true, Payload::None)).stage() == Stage::Resolve;
    let c = judge(&Node::new(NodeKind::Reg, Perm::rwxr_xr_x(), true, 0, false, Payload::Text)).stage() == Stage::Format;
    let d = judge(&Node::elf()).stage() == Stage::Format && judge(&Node::elf()).accepted();
    a && b && c && d
}

/// 语料全跑。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.total() == CORPUS && r.accept() == 2 && r.reject() == 12 && r.matched() == CORPUS;
    let b = r.eacces() == 7 && r.enoexec() == 1 && r.enoent() == 2;
    let c = r.notdir() == 1 && r.eloop() == 1;
    let d = r.rate() == 1000 && r.green() && r.gate() == Ok(());
    a && b && c && d
}

/// 语料编号。
fn slots_ok() -> bool {
    if !unique() || !notes_ok() || !table_ok() {
        return false;
    }
    let a = match slot_at(0) {
        Some(s) => s.node.kind == NodeKind::Dir && s.code == XP_EDIR,
        None => false,
    };
    let b = match slot_at(3) {
        Some(s) => s.code == 0 && s.node.payload == Payload::Elf,
        None => false,
    };
    let c = slot_at(CORPUS).is_none() && slot_at(CORPUS + 1).is_none();
    a && b && c
}

/// 汇总渲染。
fn report_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.render().as_str()
        == "execpath corpus=14 accept=2 reject=12 eacces=7 enoexec=1 enoent=2 notdir=1 eloop=1 match=14 rate=1000ppm\n";
    let e = Report::new();
    let b = e.is_empty() && e.rate() == 0 && e.gate() == Err(XP_ENOBS);
    let c = e.render().as_str()
        == "execpath corpus=0 accept=0 reject=0 eacces=0 enoexec=0 enoent=0 notdir=0 eloop=0 match=0 rate=0ppm\n";
    a && b && c
}

/// 红单显式。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, XP_EDIR))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched() == CORPUS - 1 && r.rate() == 929;
    let b = r.gate() == Err(XP_EVER) && !r.green();
    let c = run_with(Some((CORPUS, XP_EDIR))).err() == Some(XP_ESLOT);
    a && b && c
}

/// 封面。
fn cover_ok() -> bool {
    cover(CORPUS) == Ok(()) && cover(CORPUS - 1) == Err(XP_ENOBS) && cover(0) == Err(XP_ENOBS)
}

/// errno 与文案。
fn message_ok() -> bool {
    let dir_v = judge(&Node::dir());
    let a = dir_v.errno() == EACCES && strerror(EACCES) == "Permission denied";
    let b = match message(&dir_v, "./dir") {
        Ok(s) => s.as_str() == "bash: ./dir: Permission denied (directory, errno=13)\n",
        Err(_) => false,
    };
    let miss_v = judge(&Node::new(NodeKind::Reg, Perm::none(), true, 0, true, Payload::None));
    let c = miss_v.errno() == ENOENT;
    let d = match message(&miss_v, "./nope") {
        Ok(s) => s.as_str() == "bash: ./nope: No such file or directory (dangling symlink)\n",
        Err(_) => false,
    };
    let e = match message(&judge(&Node::elf()), "./prog") {
        Ok(s) => s.as_str() == "ok: ELF exec ./prog\n",
        Err(_) => false,
    };
    let f = match notdir("/etc/passwd/x") {
        Ok(v) => match message(&v, "/etc/passwd/x") {
            Ok(s) => s.as_str() == "bash: /etc/passwd/x: Not a directory (component not directory)\n",
            Err(_) => false,
        },
        Err(_) => false,
    };
    let g = message(&dir_v, "") == Err(XP_ENOENT);
    a && b && c && d && e && f && g
}

/// strerror 全表。
fn strerror_ok() -> bool {
    strerror(0) == "Success"
        && strerror(ELOOP) == "Too many levels of symbolic links"
        && strerror(ENOEXEC) == "Exec format error"
        && strerror(ENOTDIR) == "Not a directory"
        && EACCES == 13
        && ENOENT == 2
        && ENOTDIR == 20
        && ELOOP == 40
        && ENOEXEC == 8
}

/// 解析面。
fn walker_ok() -> bool {
    let mut w = Walker::new();
    let a = !w.bound() && w.steps() == 0;
    let b = match w.walk(&Node::dir()) {
        Ok(v) => v.code() == XP_EDIR,
        Err(_) => false,
    };
    let c = w.bound() && w.steps() == 1 && w.last() == XP_EDIR;
    let d = w.walk(&Node::elf()) == Err(XP_ESTATE) && w.last() == XP_ESTATE;
    a && b && c && d
}

/// 路径文本。
fn path_text_ok() -> bool {
    segments("./a.out") == 2
        && segments("/usr/bin/env") == 3
        && segments("/") == 0
        && segments("") == 0
        && segments("/tmp/../x/") == 3
        && absolute("/bin/sh")
        && !absolute("bin/sh")
        && !absolute("")
}

/// 摘要。
fn seal_ok() -> bool {
    let a = node_seal(&Node::elf()) == node_seal(&Node::elf());
    let b = node_seal(&Node::elf()) != node_seal(&Node::dir());
    let c = digest(b"exec") == digest(b"exec") && digest(b"exec") != digest(b"exec2");
    let d = node_seal(&Node::new(NodeKind::Reg, Perm::rwxr_xr_x(), false, 0, false, Payload::Elf))
        != node_seal(&Node::elf());
    a && b && c && d
}

/// 文本。
fn text_ok() -> bool {
    u32_text(0).as_str() == "0"
        && u32_text(13).as_str() == "13"
        && u32_text(929).as_str() == "929"
        && NodeKind::Dir.name() == "directory"
        && NodeKind::Reg.loadable()
        && !NodeKind::Fifo.loadable()
        && Stage::Resolve.name() == "resolve"
        && Payload::Elf.name() == "elf"
}

/// 器与面序。
fn misc_ok() -> bool {
    let a = Perm::rwxr_xr_x().bits() == 0o755 && Perm::rw_r_r().bits() == 0o644;
    let b = Perm::none().bits() == 0 && !Perm::none().exec() && !Perm::none().readable();
    let c = Node::dir().kind == NodeKind::Dir && Node::elf().payload == Payload::Elf;
    let d = MAX_LINKS == 40;
    a && b && c && d
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = kind_ok() && perm_ok() && misc_ok();
    out[1] = path_ok() && stage_ok() && path_text_ok();
    out[2] = corpus_ok() && slots_ok() && table_ok();
    out[3] = report_ok() && reds_ok() && cover_ok() && message_ok() && strerror_ok()
        && walker_ok() && seal_ok() && text_ok()
        && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11608_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11608.exec_path");

    s.add("XP01-类型闸门", kind_ok(), "static");
    s.add("XP02-权限闸门", perm_ok(), "static");
    s.add("XP03-路径闸门", path_ok(), "static");
    s.add("XP04-阶段闸门", stage_ok(), "static");
    s.add("XP05-解析文本", path_text_ok() && misc_ok(), "static");
    s.add("XP06-十四卡全跑", corpus_ok(), "static");
    s.add("XP07-语料编号", slots_ok(), "static");
    s.add("XP08-汇总渲染", report_ok(), "static");
    s.add("XP09-红单显式", reds_ok(), "static");
    s.add("XP10-封面", cover_ok(), "static");
    s.add("XP11-文案逐字", message_ok() && strerror_ok(), "static");
    s.add("XP12-解析面与摘要", walker_ok() && seal_ok() && text_ok(), "static");
    s.add("XP13-冻结重放四注入", inject_four().iter().all(|x| *x), "static");

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpus_shape() {
        assert_eq!(CASES.len(), CORPUS);
        assert_eq!(CORPUS, 14);
        assert!(unique());
        assert!(notes_ok());
        assert!(table_ok());
    }

    #[test]
    fn dir_is_eacces() {
        let v = judge(&Node::dir());
        assert_eq!(v.code(), XP_EDIR);
        assert_eq!(v.errno(), EACCES);
        assert!(!v.accepted());
        assert_eq!(strerror(EACCES), "Permission denied");
    }

    #[test]
    fn dir_without_x_still_dir() {
        let n = Node::new(NodeKind::Dir, Perm::none(), true, 0, false, Payload::None);
        assert_eq!(judge(&n).code(), XP_EDIR);
        assert_eq!(judge(&n).detail(), "directory");
    }

    #[test]
    fn nonregular_is_eacces() {
        for k in [NodeKind::Fifo, NodeKind::Sock, NodeKind::Chr, NodeKind::Blk] {
            let n = Node::new(k, Perm::rwxr_xr_x(), true, 0, false, Payload::None);
            assert_eq!(judge(&n).code(), XP_EPERM);
            assert_eq!(judge(&n).errno(), EACCES);
        }
    }

    #[test]
    fn exec_bit_required() {
        let n = Node::new(NodeKind::Reg, Perm::rw_r_r(), true, 0, false, Payload::Elf);
        assert_eq!(judge(&n).code(), XP_EPERM);
        assert_eq!(judge(&n).detail(), "exec bit clear");
    }

    #[test]
    fn noexec_mount() {
        let n = Node::new(NodeKind::Reg, Perm::rwxr_xr_x(), false, 0, false, Payload::Elf);
        assert_eq!(judge(&n).code(), XP_EPERM);
        assert_eq!(judge(&n).detail(), "noexec mount");
    }

    #[test]
    fn dangling_and_loop() {
        let d = Node::new(NodeKind::Link, Perm::rw_r_r(), true, 2, true, Payload::None);
        assert_eq!(judge(&d).code(), XP_ENOENT);
        let l = Node::new(NodeKind::Link, Perm::rwxr_xr_x(), true, MAX_LINKS + 1, false, Payload::Elf);
        assert_eq!(judge(&l).code(), XP_ELOOP);
        assert_eq!(judge(&l).errno(), ELOOP);
    }

    #[test]
    fn link_to_elf_ok() {
        let n = Node::new(NodeKind::Link, Perm::rwxr_xr_x(), true, 3, false, Payload::Elf);
        assert!(judge(&n).accepted());
        assert_eq!(judge(&n).errno(), 0);
    }

    #[test]
    fn text_exec_enoexec() {
        let n = Node::new(NodeKind::Reg, Perm::rwxr_xr_x(), true, 0, false, Payload::Text);
        assert_eq!(judge(&n).code(), XP_ENOEXEC);
        assert_eq!(judge(&n).errno(), ENOEXEC);
        assert_eq!(judge(&n).stage(), Stage::Format);
    }

    #[test]
    fn notdir_path() {
        let v = match notdir("/etc/passwd/x") {
            Ok(x) => x,
            Err(_) => return,
        };
        assert_eq!(v.code(), XP_ENOTDIR);
        assert_eq!(v.errno(), ENOTDIR);
    }

    #[test]
    fn report_counts() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(r.accept(), 2);
        assert_eq!(r.reject(), 12);
        assert_eq!(r.eacces(), 7);
        assert_eq!(r.enoexec(), 1);
        assert_eq!(r.enoent(), 2);
        assert_eq!(r.notdir(), 1);
        assert_eq!(r.eloop(), 1);
        assert_eq!(r.gate(), Ok(()));
    }

    #[test]
    fn red_override() {
        let r = match run_with(Some((1, XP_EPERM))) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(r.rate(), 929);
        assert_eq!(r.gate(), Err(XP_EVER));
    }

    #[test]
    fn message_dir() {
        let m = match message(&judge(&Node::dir()), "./dir") {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(m.as_str(), "bash: ./dir: Permission denied (directory, errno=13)\n");
    }

    #[test]
    fn walker_once() {
        let mut w = Walker::new();
        assert!(w.walk(&Node::elf()).is_ok());
        assert_eq!(w.walk(&Node::elf()), Err(XP_ESTATE));
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert!(l.sample(SAMPLES).is_none());
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = match l.freeze() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert!(l.is_frozen());
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(XP_EBAL));
    }

    #[test]
    fn injections() {
        let out = inject_four();
        assert!(out[0]);
        assert!(out[1]);
        assert!(out[2]);
        assert!(out[3]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11608_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }
}
