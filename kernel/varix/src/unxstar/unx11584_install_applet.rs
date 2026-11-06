//! UNX-F11584 --install 软链全 applet 与 argv[0] 派发判据
//!
//! `busybox --install -s /bin` 做的事是把 40 个 applet 名各造一条软链指向
//! `/bin/busybox`，之后每次调用都靠 argv[0] 的基名回派。这条路径比「写 40 个
//! 小可执行文件」省空间，但把三类风险一次性引进来：
//!
//! ```text
//! 覆盖  目标位置已有同名普通文件 / 目录 / 指向别处的软链
//! 幂等  第二次 --install 必须全部判「已就位」，不能重造也不能报错
//! 落盘  计划通过后写盘要么全成、要么在第一处冲突停住并报出该条错误码
//! ```
//!
//! 本模块因此不直接「装」，而是先建**虚拟盘**（定容表）与**链接计划**：
//! 计划逐条给出动作（新建 / 保持 / 替换 / 冲突），落盘只执行前三类，任何
//! 冲突都在落盘前就被 `first_conflict()` 摘出来。这样同一套判据既能验
//! 「全新盘装 40 条」，也能验「半装盘补装」「被占用盘拒绝」「-f 强制替换」。
//!
//! 闸门次序：目录存在 → 目录类型 → 路径字节 → 逐 applet 查位 → 冲突分类 → 计划落账 → 落盘。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本 14 格
//! （8 成 6 败）在 `audit` 逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf990;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 路径容量。
pub const PATH_CAP: usize = 64;

/// 虚拟盘节点容量。
pub const FS_CAP: usize = 64;

/// 链接计划容量。
pub const PLAN_CAP: usize = 48;

/// 待装 applet 数。
pub const APPLETS: usize = 40;

/// 多调二进制名。
pub const BUSYBOX: &str = "busybox";

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 名字/路径非法。
pub const IN_EBYTE: u32 = SEG_BASE + 0x01;
/// 安装目录不存在。
pub const IN_ENODIR: u32 = SEG_BASE + 0x02;
/// 安装目录不是目录。
pub const IN_ENOTDIR: u32 = SEG_BASE + 0x03;
/// 同名普通文件占位。
pub const IN_EEXIST: u32 = SEG_BASE + 0x04;
/// 已有软链指向他处。
pub const IN_EWRONG: u32 = SEG_BASE + 0x05;
/// 同名目录占位。
pub const IN_EDIR: u32 = SEG_BASE + 0x06;
/// 容量越界。
pub const IN_ECAP: u32 = SEG_BASE + 0x07;
/// 选项/调用形态非法。
pub const IN_EOPT: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const IN_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const IN_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const IN_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const IN_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const IN_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 虚拟盘装载。
pub const OP_FS: u8 = 0x01;
/// 路径拼装。
pub const OP_PATH: u8 = 0x02;
/// 计划生成。
pub const OP_PLAN: u8 = 0x03;
/// 落盘。
pub const OP_APPLY: u8 = 0x04;
/// 目标渲染。
pub const OP_TARGET: u8 = 0x05;
/// 选项。
pub const OP_OPT: u8 = 0x06;

// ---------------------------------------------------------------------------
// 哈希
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

// ---------------------------------------------------------------------------
// 字节助手
// ---------------------------------------------------------------------------

/// 可打印（0x21..=0x7e）。
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

/// 字节拼 String。
fn text_of(s: &[u8]) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    while i < s.len() {
        if let Some(b) = s.get(i) {
            out.push(*b as char);
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 安装名表
// ---------------------------------------------------------------------------

/// 待装 applet 名（C 序 40 条，与 `--list` 同序）。
pub const INSTALL_NAMES: [&str; APPLETS] = [
    "ash", "awk", "cat", "chmod", "chown", "cp", "cut", "date", "dd", "df",
    "du", "echo", "expr", "find", "grep", "head", "kill", "ln", "ls", "mkdir",
    "mount", "mv", "printf", "ps", "rm", "rmdir", "sed", "sh", "sleep", "sort",
    "stat", "tail", "tar", "tee", "test", "touch", "tr", "umount", "uniq", "wc",
];

// ---------------------------------------------------------------------------
// 路径
// ---------------------------------------------------------------------------

/// 定容路径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Path {
    bytes: [u8; PATH_CAP],
    len: usize,
}

impl Path {
    /// 空白。
    pub const fn blank() -> Path {
        Path { bytes: [0u8; PATH_CAP], len: 0 }
    }

    /// 构造：空 / 超长 / 非打印 / 含 NUL 一律拒（NUL 属于非打印）。
    pub fn new(s: &[u8]) -> Result<Path, u32> {
        if s.is_empty() {
            return Err(IN_EBYTE);
        }
        if s.len() > PATH_CAP {
            return Err(IN_EBYTE);
        }
        if !printable(s) {
            return Err(IN_EBYTE);
        }
        let mut p = Path::blank();
        let mut i = 0usize;
        while i < s.len() {
            match (s.get(i), p.bytes.get_mut(i)) {
                (Some(src), Some(dst)) => *dst = *src,
                _ => return Err(IN_EBYTE),
            }
            i += 1;
        }
        p.len = s.len();
        Ok(p)
    }

    /// 拼一段：`dir` + `/` + `name`（`dir` 已以 `/` 结尾时不重复补）。
    pub fn join(dir: &[u8], name: &[u8]) -> Result<Path, u32> {
        if dir.is_empty() || name.is_empty() {
            return Err(IN_EBYTE);
        }
        if !printable(dir) || !printable(name) {
            return Err(IN_EBYTE);
        }
        let mut p = Path::blank();
        let mut n = 0usize;
        let mut i = 0usize;
        while i < dir.len() {
            match (dir.get(i), p.bytes.get_mut(n)) {
                (Some(src), Some(dst)) => *dst = *src,
                _ => return Err(IN_EBYTE),
            }
            n += 1;
            i += 1;
        }
        let tail_is_slash = dir.get(dir.len().saturating_sub(1)) == Some(&b'/');
        if !tail_is_slash {
            if n >= PATH_CAP {
                return Err(IN_EBYTE);
            }
            if let Some(dst) = p.bytes.get_mut(n) {
                *dst = b'/';
            }
            n += 1;
        }
        let mut k = 0usize;
        while k < name.len() {
            if n >= PATH_CAP {
                return Err(IN_EBYTE);
            }
            match (name.get(k), p.bytes.get_mut(n)) {
                (Some(src), Some(dst)) => *dst = *src,
                _ => return Err(IN_EBYTE),
            }
            n += 1;
            k += 1;
        }
        p.len = n;
        Ok(p)
    }

    /// 字节。
    pub fn bytes(&self) -> &[u8] {
        self.bytes.get(0..self.len).unwrap_or(&[])
    }

    /// 文本。
    pub fn text(&self) -> String {
        text_of(self.bytes())
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 是否同路径。
    pub fn named(&self, s: &[u8]) -> bool {
        eq_bytes(self.bytes(), s)
    }

    /// 末段基名。
    pub fn base(&self) -> &[u8] {
        let mut last: Option<usize> = None;
        let mut i = 0usize;
        while i < self.len {
            if let Some(b) = self.bytes.get(i) {
                if *b == b'/' {
                    last = Some(i);
                }
            }
            i += 1;
        }
        match last {
            Some(p) => self.bytes.get(p.saturating_add(1)..self.len).unwrap_or(&[]),
            None => self.bytes(),
        }
    }
}

// ---------------------------------------------------------------------------
// 虚拟盘
// ---------------------------------------------------------------------------

/// 虚拟盘节点类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 普通文件。
    File,
    /// 目录。
    Dir,
    /// 软链。
    Link,
}

/// 虚拟盘节点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    /// 路径。
    pub path: Path,
    /// 类型。
    pub kind: Kind,
    /// 软链目标（非软链为空）。
    pub target: Path,
}

impl Entry {
    /// 目录。
    pub fn dir(p: &[u8]) -> Result<Entry, u32> {
        Ok(Entry { path: Path::new(p)?, kind: Kind::Dir, target: Path::blank() })
    }

    /// 普通文件。
    pub fn file(p: &[u8]) -> Result<Entry, u32> {
        Ok(Entry { path: Path::new(p)?, kind: Kind::File, target: Path::blank() })
    }

    /// 软链。
    pub fn link(p: &[u8], t: &[u8]) -> Result<Entry, u32> {
        Ok(Entry { path: Path::new(p)?, kind: Kind::Link, target: Path::new(t)? })
    }

    /// 一行文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        match self.kind {
            Kind::File => s.push('-'),
            Kind::Dir => s.push('d'),
            Kind::Link => s.push('l'),
        }
        s.push(' ');
        s.push_str(self.path.text().as_str());
        if self.kind == Kind::Link {
            s.push_str(" -> ");
            s.push_str(self.target.text().as_str());
        }
        s
    }
}

/// 虚拟盘。
#[derive(Clone, Copy, Debug)]
pub struct Fs {
    entries: [Entry; FS_CAP],
    len: usize,
}

impl Fs {
    /// 空盘。
    pub const fn new() -> Fs {
        let blank = Entry { path: Path::blank(), kind: Kind::File, target: Path::blank() };
        Fs { entries: [blank; FS_CAP], len: 0 }
    }

    /// 节点数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 追加（同路径覆盖）。
    pub fn put(&mut self, e: Entry) -> Result<(), u32> {
        let mut i = 0usize;
        while i < self.len {
            match self.entries.get(i) {
                Some(old) => {
                    if old.path.named(e.path.bytes()) {
                        if let Some(dst) = self.entries.get_mut(i) {
                            *dst = e;
                        }
                        return Ok(());
                    }
                }
                None => return Err(IN_ECAP),
            }
            i += 1;
        }
        if self.len >= FS_CAP {
            return Err(IN_ECAP);
        }
        if let Some(dst) = self.entries.get_mut(self.len) {
            *dst = e;
        }
        self.len += 1;
        Ok(())
    }

    /// 查节点。
    pub fn get(&self, p: &[u8]) -> Option<Entry> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(e) = self.entries.get(i) {
                if e.path.named(p) {
                    return Some(*e);
                }
            }
            i += 1;
        }
        None
    }

    /// 某路径的类型。
    pub fn kind_at(&self, p: &[u8]) -> Option<Kind> {
        self.get(p).map(|e| e.kind)
    }

    /// 是否目录。
    pub fn is_dir(&self, p: &[u8]) -> bool {
        self.kind_at(p) == Some(Kind::Dir)
    }

    /// 软链数。
    pub fn link_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(e) = self.entries.get(i) {
                if e.kind == Kind::Link {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 选项
// ---------------------------------------------------------------------------

/// 安装选项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opt {
    /// `-f` 强制替换占位。
    pub replace: bool,
    /// `-r` 软链目标用相对名。
    pub relative: bool,
    /// `-n` 只出计划不落盘。
    pub dry: bool,
}

impl Opt {
    /// 缺省（不替换、绝对目标、真落盘）。
    pub const fn new() -> Opt {
        Opt { replace: false, relative: false, dry: false }
    }

    /// 文本（空格分隔，空则为 `plain`）。
    pub fn text(&self) -> String {
        let mut s = String::new();
        if self.replace {
            s.push_str("force");
        }
        if self.relative {
            if !s.is_empty() {
                s.push(' ');
            }
            s.push_str("relative");
        }
        if self.dry {
            if !s.is_empty() {
                s.push(' ');
            }
            s.push_str("dry");
        }
        if s.is_empty() {
            s.push_str("plain");
        }
        s
    }
}

/// 解析安装命令行：必须出现 `--install`，选项之外不接受操作数。
pub fn parse_argv(args: &[&[u8]]) -> Result<Opt, u32> {
    let mut o = Opt::new();
    let mut install = false;
    let mut i = 0usize;
    while i < args.len() {
        let a = match args.get(i) {
            Some(v) => *v,
            None => return Err(IN_EOPT),
        };
        if eq_bytes(a, b"--install") {
            install = true;
        } else if eq_bytes(a, b"-s") {
            // 软链模式（本计划器恒为软链，仅记录出现）。
        } else if eq_bytes(a, b"-f") {
            o.replace = true;
        } else if eq_bytes(a, b"-r") {
            o.relative = true;
        } else if eq_bytes(a, b"-n") {
            o.dry = true;
        } else {
            return Err(IN_EOPT);
        }
        i += 1;
    }
    if !install {
        return Err(IN_EOPT);
    }
    Ok(o)
}

// ---------------------------------------------------------------------------
// 链接计划
// ---------------------------------------------------------------------------

/// 单条动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// 新建软链。
    Create,
    /// 已就位（同一目标）。
    Keep,
    /// 强制替换占位。
    Replace,
    /// 冲突，不落盘。
    Conflict,
}

impl Action {
    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            Action::Create => "create",
            Action::Keep => "keep",
            Action::Replace => "replace",
            Action::Conflict => "conflict",
        }
    }
}

/// 计划中的一条。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Link {
    /// 软链路径。
    pub path: Path,
    /// 目标。
    pub target: Path,
    /// 动作。
    pub action: Action,
    /// 冲突码（非冲突为 0）。
    pub code: u32,
}

impl Link {
    /// 一行文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        s.push_str(self.action.text());
        s.push(' ');
        s.push_str(self.path.text().as_str());
        s.push_str(" -> ");
        s.push_str(self.target.text().as_str());
        s
    }
}

/// 链接计划。
#[derive(Clone, Copy, Debug)]
pub struct Plan {
    items: [Link; PLAN_CAP],
    len: usize,
    dir: Path,
    target: Path,
    conflicts: usize,
    created: usize,
    kept: usize,
    replaced: usize,
}

impl Plan {
    /// 空计划。
    pub const fn new() -> Plan {
        let blank = Link {
            path: Path::blank(),
            target: Path::blank(),
            action: Action::Conflict,
            code: 0,
        };
        Plan {
            items: [blank; PLAN_CAP],
            len: 0,
            dir: Path::blank(),
            target: Path::blank(),
            conflicts: 0,
            created: 0,
            kept: 0,
            replaced: 0,
        }
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取条。
    pub fn at(&self, i: usize) -> Option<Link> {
        if i >= self.len {
            return None;
        }
        self.items.get(i).copied()
    }

    /// 安装目录。
    pub fn dir(&self) -> Path {
        self.dir
    }

    /// 目标。
    pub fn target(&self) -> Path {
        self.target
    }

    /// 冲突条数。
    pub fn conflicts(&self) -> usize {
        self.conflicts
    }

    /// 新建条数。
    pub fn created(&self) -> usize {
        self.created
    }

    /// 保持条数。
    pub fn kept(&self) -> usize {
        self.kept
    }

    /// 替换条数。
    pub fn replaced(&self) -> usize {
        self.replaced
    }

    /// 首个冲突码。
    pub fn first_conflict(&self) -> Option<u32> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(l) = self.items.get(i) {
                if l.action == Action::Conflict {
                    return Some(l.code);
                }
            }
            i += 1;
        }
        None
    }

    /// 是否干净（无冲突）。
    pub fn clean(&self) -> bool {
        self.conflicts == 0 && self.len == APPLETS
    }

    /// 整表文本（逐行，`\n` 结尾）。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(l) = self.items.get(i) {
                s.push_str(l.text().as_str());
                s.push('\n');
            }
            i += 1;
        }
        s
    }

    /// 落盘：新建/替换写盘，保持跳过，冲突立即报码。
    pub fn apply_to(&self, fs: &mut Fs) -> Result<Stats, u32> {
        let mut st = Stats::blank();
        let mut i = 0usize;
        while i < self.len {
            let l = match self.items.get(i) {
                Some(v) => *v,
                None => return Err(IN_ECAP),
            };
            match l.action {
                Action::Create => {
                    fs.put(Entry::link(l.path.bytes(), l.target.bytes())?)?;
                    st.created += 1;
                }
                Action::Keep => {
                    st.kept += 1;
                }
                Action::Replace => {
                    fs.put(Entry::link(l.path.bytes(), l.target.bytes())?)?;
                    st.replaced += 1;
                }
                Action::Conflict => return Err(l.code),
            }
            i += 1;
        }
        Ok(st)
    }
}

/// 落盘统计。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stats {
    /// 新建。
    pub created: usize,
    /// 保持。
    pub kept: usize,
    /// 替换。
    pub replaced: usize,
}

impl Stats {
    /// 空白。
    pub const fn blank() -> Stats {
        Stats { created: 0, kept: 0, replaced: 0 }
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        s.push_str("created=");
        push_u64(&mut s, self.created as u64);
        s.push_str(" kept=");
        push_u64(&mut s, self.kept as u64);
        s.push_str(" replaced=");
        push_u64(&mut s, self.replaced as u64);
        s
    }
}

/// 冲突判定：查位 → 分类。
fn classify(fs: &Fs, path: &Path, target: &Path, opt: &Opt) -> (Action, u32) {
    match fs.get(path.bytes()) {
        None => (Action::Create, 0),
        Some(e) => match e.kind {
            Kind::Dir => (Action::Conflict, IN_EDIR),
            Kind::File => {
                if opt.replace {
                    (Action::Replace, 0)
                } else {
                    (Action::Conflict, IN_EEXIST)
                }
            }
            Kind::Link => {
                if eq_bytes(e.target.bytes(), target.bytes()) {
                    (Action::Keep, 0)
                } else if opt.replace {
                    (Action::Replace, 0)
                } else {
                    (Action::Conflict, IN_EWRONG)
                }
            }
        },
    }
}

/// 生成计划：安装目录必须是盘上已有的目录。
pub fn plan(fs: &Fs, dir: &[u8], opt: &Opt) -> Result<Plan, u32> {
    let d = Path::new(dir)?;
    match fs.kind_at(d.bytes()) {
        None => return Err(IN_ENODIR),
        Some(Kind::Dir) => {}
        Some(_) => return Err(IN_ENOTDIR),
    }
    let mut p = Plan::new();
    p.dir = d;
    let target = if opt.relative {
        Path::new(BUSYBOX.as_bytes())?
    } else {
        Path::join(d.bytes(), BUSYBOX.as_bytes())?
    };
    p.target = target;
    let mut i = 0usize;
    while i < APPLETS {
        if p.len >= PLAN_CAP {
            return Err(IN_ECAP);
        }
        let name = match INSTALL_NAMES.get(i) {
            Some(v) => v.as_bytes(),
            None => return Err(IN_EBYTE),
        };
        let link_path = Path::join(d.bytes(), name)?;
        let (action, code) = classify(fs, &link_path, &target, opt);
        match action {
            Action::Create => p.created += 1,
            Action::Keep => p.kept += 1,
            Action::Replace => p.replaced += 1,
            Action::Conflict => p.conflicts += 1,
        }
        if let Some(dst) = p.items.get_mut(p.len) {
            *dst = Link { path: link_path, target, action, code };
        }
        p.len += 1;
        i += 1;
    }
    Ok(p)
}

/// 计划 + 干净性检查：有冲突即返回首个冲突码。
pub fn plan_clean(fs: &Fs, dir: &[u8], opt: &Opt) -> Result<u32, u32> {
    let p = plan(fs, dir, opt)?;
    match p.first_conflict() {
        Some(c) => Err(c),
        None => Ok(p.len() as u32),
    }
}

/// 全新盘：只有 `/bin` 与 `/bin/busybox`。
pub fn base_fs() -> Result<Fs, u32> {
    let mut fs = Fs::new();
    fs.put(Entry::dir(b"/bin")?)?;
    fs.put(Entry::file(b"/bin/busybox")?)?;
    Ok(fs)
}

/// 装好 40 条软链的盘。
pub fn installed_fs() -> Result<Fs, u32> {
    let mut fs = base_fs()?;
    let p = plan(&fs, b"/bin", &Opt::new())?;
    if let Some(c) = p.first_conflict() {
        return Err(c);
    }
    p.apply_to(&mut fs)?;
    Ok(fs)
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
            return Err(IN_ESLOT);
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
            return Err(IN_EBAL);
        }
        const VALS: [u32; 8] = [2, 7, 40, 40, 40, 0, 12, 1];
        const CODES: [u32; 6] = [
            IN_ENODIR,
            IN_ENOTDIR,
            IN_EEXIST,
            IN_EWRONG,
            IN_EDIR,
            IN_EBYTE,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(IN_EBAL),
            };
            if s.slot as usize != i {
                return Err(IN_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(IN_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(IN_EVALUE);
                        }
                    }
                    None => return Err(IN_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(IN_EVALUE);
                        }
                    }
                    None => return Err(IN_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(IN_EBAL);
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
            return Err(IN_EBAL);
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
            Err(IN_ESEAL)
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
fn cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<u32, u32>` → 样本。
fn cell32(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 盘上放一个占位文件再规划出冲突数。
fn conflict_with(entry: Result<Entry, u32>) -> Result<usize, u32> {
    let e = entry?;
    let mut fs = base_fs()?;
    fs.put(e)?;
    let p = plan(&fs, b"/bin", &Opt::new())?;
    Ok(p.conflicts())
}

/// 账本组装。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(cell(0, OP_FS, base_fs().map(|f| f.len())));

    let _ = l.register(cell(1, OP_PATH, Path::join(b"/bin", b"ls").map(|p| p.len())));

    let _ = l.register(cell(
        2,
        OP_PLAN,
        base_fs().and_then(|f| plan(&f, b"/bin", &Opt::new())).map(|p| p.len()),
    ));

    let _ = l.register(cell(
        3,
        OP_PLAN,
        base_fs().and_then(|f| plan(&f, b"/bin", &Opt::new())).map(|p| p.created()),
    ));

    let _ = l.register(cell(
        4,
        OP_APPLY,
        installed_fs()
            .and_then(|f| plan(&f, b"/bin", &Opt::new()))
            .map(|p| p.kept()),
    ));

    let _ = l.register(cell(
        5,
        OP_APPLY,
        installed_fs().and_then(|mut f| {
            let p = plan(&f, b"/bin", &Opt::new())?;
            p.apply_to(&mut f).map(|s| s.created)
        }),
    ));

    let _ = l.register(cell(
        6,
        OP_TARGET,
        base_fs()
            .and_then(|f| plan(&f, b"/bin", &Opt::new()))
            .map(|p| p.target().len()),
    ));

    let _ = l.register(cell(
        7,
        OP_PLAN,
        conflict_with(Entry::file(b"/bin/ls")),
    ));

    let _ = l.register(cell32(
        8,
        OP_PLAN,
        base_fs()
            .and_then(|f| plan_clean(&f, b"/nope", &Opt::new()))
            .map(|n| n),
    ));

    let _ = l.register(cell32(
        9,
        OP_PLAN,
        base_fs()
            .and_then(|mut f| f.put(Entry::file(b"/opt")?).map(|_| f))
            .and_then(|f| plan_clean(&f, b"/opt", &Opt::new())),
    ));

    let _ = l.register(cell32(
        10,
        OP_PLAN,
        base_fs()
            .and_then(|mut f| f.put(Entry::file(b"/bin/ls")?).map(|_| f))
            .and_then(|f| plan_clean(&f, b"/bin", &Opt::new())),
    ));

    let _ = l.register(cell32(
        11,
        OP_PLAN,
        base_fs()
            .and_then(|mut f| f.put(Entry::link(b"/bin/ls", b"/bin/sh")?).map(|_| f))
            .and_then(|f| plan_clean(&f, b"/bin", &Opt::new())),
    ));

    let _ = l.register(cell32(
        12,
        OP_PLAN,
        base_fs()
            .and_then(|mut f| f.put(Entry::dir(b"/bin/ls")?).map(|_| f))
            .and_then(|f| plan_clean(&f, b"/bin", &Opt::new())),
    ));

    let _ = l.register(cell32(
        13,
        OP_PLAN,
        base_fs().and_then(|f| plan_clean(&f, &[0x01u8], &Opt::new())),
    ));

    l
}

// ---------------------------------------------------------------------------
// 判据面
// ---------------------------------------------------------------------------

/// 虚拟盘装载。
fn fs_ok() -> bool {
    let f = match base_fs() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if f.len() != 2 {
        return false;
    }
    if !f.is_dir(b"/bin") || f.is_dir(b"/bin/busybox") {
        return false;
    }
    if f.kind_at(b"/bin/busybox") != Some(Kind::File) {
        return false;
    }
    if f.get(b"/bin/ls").is_some() || f.link_count() != 0 {
        return false;
    }
    let mut g = Fs::new();
    let d = match Entry::dir(b"/etc") {
        Ok(v) => v,
        Err(_) => return false,
    };
    if g.put(d).is_err() {
        return false;
    }
    let again = match Entry::dir(b"/etc") {
        Ok(v) => v,
        Err(_) => return false,
    };
    if g.put(again).is_err() || g.len() != 1 {
        return false;
    }
    let l = match Entry::link(b"/etc/a", b"/bin/busybox") {
        Ok(v) => v,
        Err(_) => return false,
    };
    if g.put(l).is_err() || g.link_count() != 1 {
        return false;
    }
    g.kind_at(b"/etc/a") == Some(Kind::Link)
        && g.get(b"/etc/a").map(|e| e.target.named(b"/bin/busybox")) == Some(true)
        && g.get(b"/etc/a").map(|e| e.text().as_str() == "l /etc/a -> /bin/busybox")
            == Some(true)
}

/// 路径拼装。
fn path_ok() -> bool {
    let a = match Path::join(b"/bin", b"ls") {
        Ok(v) => v,
        Err(_) => return false,
    };
    if a.text().as_str() != "/bin/ls" || a.len() != 7 {
        return false;
    }
    if a.base() != b"ls" || !a.named(b"/bin/ls") {
        return false;
    }
    let b = match Path::join(b"/", b"ls") {
        Ok(v) => v,
        Err(_) => return false,
    };
    if b.text().as_str() != "/ls" {
        return false;
    }
    let c = match Path::join(b"/bin/", b"ls") {
        Ok(v) => v,
        Err(_) => return false,
    };
    if c.text().as_str() != "/bin/ls" {
        return false;
    }
    let d = Path::join(b"/bin", b"").err() == Some(IN_EBYTE);
    let e = Path::join(b"", b"ls").err() == Some(IN_EBYTE);
    let f = Path::new(&[b'z'; 65]).err() == Some(IN_EBYTE);
    let g = Path::new(&[0x01]).err() == Some(IN_EBYTE);
    let h = Path::new(b"/bin/ls").map(|p| p.base() == b"ls") == Ok(true);
    a.len() == 7 && d && e && f && g && h
}

/// 目录闸。
fn dir_gate_ok() -> bool {
    let f = match base_fs() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = plan(&f, b"/nope", &Opt::new()).err() == Some(IN_ENODIR);
    let b = plan(&f, b"/bin/busybox", &Opt::new()).err() == Some(IN_ENOTDIR);
    let c = plan(&f, &[0x02u8], &Opt::new()).err() == Some(IN_EBYTE);
    let d = plan(&Fs::new(), b"/bin", &Opt::new()).err() == Some(IN_ENODIR);
    let e = Path::new(b"").err() == Some(IN_EBYTE);
    a && b && c && d && e
}

/// 全 applet 计划。
fn full_plan_ok() -> bool {
    let f = match base_fs() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let p = match plan(&f, b"/bin", &Opt::new()) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if p.len() != APPLETS || p.is_empty() {
        return false;
    }
    if p.created() != APPLETS || p.kept() != 0 || p.replaced() != 0 || p.conflicts() != 0 {
        return false;
    }
    if !p.clean() || p.first_conflict().is_some() {
        return false;
    }
    if plan_clean(&f, b"/bin", &Opt::new()) != Ok(40) {
        return false;
    }
    let first = match p.at(0) {
        Some(v) => v,
        None => return false,
    };
    if first.action != Action::Create {
        return false;
    }
    if first.path.text().as_str() != "/bin/ash" {
        return false;
    }
    if p.at(40).is_some() {
        return false;
    }
    p.at(39).map(|l| l.path.text().as_str() == "/bin/wc") == Some(true)
}

/// 软链目标。
fn target_ok() -> bool {
    let f = match base_fs() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let abs = match plan(&f, b"/bin", &Opt::new()) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if abs.target().text().as_str() != "/bin/busybox" {
        return false;
    }
    if abs.target().len() != 12 {
        return false;
    }
    let rel_opt = Opt { replace: false, relative: true, dry: false };
    let rel = match plan(&f, b"/bin", &rel_opt) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if rel.target().text().as_str() != "busybox" {
        return false;
    }
    if rel.at(0).map(|l| l.target.named(b"busybox")) != Some(true) {
        return false;
    }
    let line = match abs.at(0) {
        Some(l) => l.text(),
        None => return false,
    };
    line.as_str() == "create /bin/ash -> /bin/busybox"
}

/// 幂等重规划。
fn idempotent_ok() -> bool {
    let f = match installed_fs() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if f.link_count() != APPLETS || f.len() != 2 + APPLETS {
        return false;
    }
    let p = match plan(&f, b"/bin", &Opt::new()) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if p.len() != APPLETS || p.kept() != APPLETS || p.created() != 0 || p.conflicts() != 0 {
        return false;
    }
    let g = match f.get(b"/bin/ls") {
        Some(v) => v,
        None => return false,
    };
    g.kind == Kind::Link && g.target.named(b"/bin/busybox")
}

/// 落盘统计。
fn apply_ok() -> bool {
    let mut f = match base_fs() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let p = match plan(&f, b"/bin", &Opt::new()) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let st = match p.apply_to(&mut f) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if st.created != APPLETS || st.kept != 0 || st.replaced != 0 || st.text().as_str() != "created=40 kept=0 replaced=0" {
        return false;
    }
    if f.link_count() != APPLETS {
        return false;
    }
    let p2 = match plan(&f, b"/bin", &Opt::new()) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let st2 = match p2.apply_to(&mut f) {
        Ok(v) => v,
        Err(_) => return false,
    };
    st2.created == 0 && st2.kept == APPLETS && f.link_count() == APPLETS
}

/// 冲突拒绝。
fn conflict_ok() -> bool {
    let a = conflict_with(Entry::file(b"/bin/ls")) == Ok(1);
    let b = conflict_with(Entry::link(b"/bin/ls", b"/bin/sh")) == Ok(1);
    let c = conflict_with(Entry::dir(b"/bin/ls")) == Ok(1);
    let f = match base_fs() {
        Ok(mut v) => {
            let e = match Entry::file(b"/bin/ls") {
                Ok(e) => e,
                Err(_) => return false,
            };
            if v.put(e).is_err() {
                return false;
            }
            v
        }
        Err(_) => return false,
    };
    let d = plan(&f, b"/bin", &Opt::new()).map(|p| p.first_conflict()) == Ok(Some(IN_EEXIST));
    let g = plan_clean(&f, b"/bin", &Opt::new()).err() == Some(IN_EEXIST);
    let mut h = f;
    let p = match plan(&h, b"/bin", &Opt::new()) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let apply = p.apply_to(&mut h).err() == Some(IN_EEXIST);
    a && b && c && d && g && apply
}

/// 强制替换。
fn replace_ok() -> bool {
    let mut f = match base_fs() {
        Ok(mut v) => {
            let e = match Entry::file(b"/bin/ls") {
                Ok(e) => e,
                Err(_) => return false,
            };
            if v.put(e).is_err() {
                return false;
            }
            v
        }
        Err(_) => return false,
    };
    let opt = Opt { replace: true, relative: false, dry: false };
    let p = match plan(&f, b"/bin", &opt) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if p.replaced() != 1 || p.conflicts() != 0 || !p.clean() {
        return false;
    }
    let st = match p.apply_to(&mut f) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if st.replaced != 1 || st.created != APPLETS - 1 {
        return false;
    }
    let e = match f.get(b"/bin/ls") {
        Some(v) => v,
        None => return false,
    };
    if e.kind != Kind::Link || !e.target.named(b"/bin/busybox") {
        return false;
    }
    let again = match plan(&f, b"/bin", &Opt::new()) {
        Ok(v) => v,
        Err(_) => return false,
    };
    again.conflicts() == 0 && again.kept() == APPLETS
}

/// 相对目标。
fn relative_ok() -> bool {
    let f = match base_fs() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let opt = Opt { replace: false, relative: true, dry: false };
    let p = match plan(&f, b"/bin", &opt) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if p.created() != APPLETS || p.target().text().as_str() != "busybox" {
        return false;
    }
    let mut fs = f;
    if p.apply_to(&mut fs).is_err() {
        return false;
    }
    let abs_opt = Opt::new();
    let abs = match plan(&fs, b"/bin", &abs_opt) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if abs.conflicts() != APPLETS || abs.first_conflict() != Some(IN_EWRONG) {
        return false;
    }
    opt.text().as_str() == "relative" && abs_opt.text().as_str() == "plain"
}

/// 选项解析。
fn option_ok() -> bool {
    let a = parse_argv(&[b"--install"]).map(|o| o.replace) == Ok(false);
    let b = parse_argv(&[b"--install", b"-f"]).map(|o| o.replace) == Ok(true);
    let c = parse_argv(&[b"--install", b"-r", b"-n"]).map(|o| o.relative && o.dry) == Ok(true);
    let d = parse_argv(&[b"-f"]).err() == Some(IN_EOPT);
    let e = parse_argv(&[b"--install", b"-x"]).err() == Some(IN_EOPT);
    let f = parse_argv(&[b"--install", b"/bin"]).err() == Some(IN_EOPT);
    let g = parse_argv(&[b"--install", b"-s"]).map(|o| o.replace) == Ok(false);
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
    if l2.replay() != Ok(root) {
        return false;
    }
    if l2.register(Sample::blank()) != Err(IN_ESLOT) {
        return false;
    }
    l2.rejects() == 1
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    if let Some(dst) = out.get_mut(0) {
        *dst = fs_ok() && path_ok() && dir_gate_ok();
    }
    if let Some(dst) = out.get_mut(1) {
        *dst = full_plan_ok() && target_ok();
    }
    if let Some(dst) = out.get_mut(2) {
        *dst = idempotent_ok() && apply_ok() && replace_ok();
    }
    if let Some(dst) = out.get_mut(3) {
        *dst = conflict_ok() && relative_ok() && option_ok() && ledger_ok();
    }
    out
}

/// 判据集合（13 项）。
pub fn run_unx11584_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11584.install");

    s.add("IN01-虚拟盘装载", fs_ok(), "static");
    s.add("IN02-路径拼装", path_ok(), "static");
    s.add("IN03-目录闸", dir_gate_ok(), "static");
    s.add("IN04-全 applet 计划", full_plan_ok(), "static");
    s.add("IN05-软链目标", target_ok(), "static");
    s.add("IN06-幂等重规划", idempotent_ok(), "static");
    s.add("IN07-落盘统计", apply_ok(), "static");
    s.add("IN08-冲突拒绝", conflict_ok(), "static");
    s.add("IN09-强制替换", replace_ok(), "static");
    s.add("IN10-相对目标", relative_ok(), "static");
    s.add("IN11-选项解析", option_ok(), "static");
    s.add("IN12-账本对账", build_ledger().audit() == Ok(()), "static");
    s.add(
        "IN13-冻结重放四注入",
        {
            let four = inject_four();
            let mut all = true;
            let mut i = 0usize;
            while i < four.len() {
                if let Some(v) = four.get(i) {
                    if !*v {
                        all = false;
                    }
                }
                i += 1;
            }
            all
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_file(p: &[u8]) -> Fs {
        let mut f = base_fs().expect("f");
        f.put(Entry::file(p).expect("e")).expect("put");
        f
    }

    #[test]
    fn fs_shape() {
        let f = base_fs().expect("f");
        assert_eq!(f.len(), 2);
        assert!(f.is_dir(b"/bin"));
        assert_eq!(f.kind_at(b"/bin/busybox"), Some(Kind::File));
        assert_eq!(f.link_count(), 0);
        assert_eq!(f.get(b"/bin/ls"), None);
    }

    #[test]
    fn path_join_forms() {
        assert_eq!(Path::join(b"/bin", b"ls").map(|p| p.text()), Ok(String::from("/bin/ls")));
        assert_eq!(Path::join(b"/", b"ls").map(|p| p.text()), Ok(String::from("/ls")));
        assert_eq!(Path::join(b"/bin/", b"ls").map(|p| p.text()), Ok(String::from("/bin/ls")));
        assert_eq!(Path::join(b"/bin", b"").err(), Some(IN_EBYTE));
        assert_eq!(Path::new(&[b'z'; 65]).err(), Some(IN_EBYTE));
    }

    #[test]
    fn plan_is_full() {
        let f = base_fs().expect("f");
        let p = plan(&f, b"/bin", &Opt::new()).expect("p");
        assert_eq!(p.len(), APPLETS);
        assert_eq!(p.created(), APPLETS);
        assert!(p.clean());
        assert_eq!(p.at(0).map(|l| l.path.text()), Some(String::from("/bin/ash")));
        assert_eq!(p.at(39).map(|l| l.path.text()), Some(String::from("/bin/wc")));
        assert_eq!(p.at(40), None);
    }

    #[test]
    fn plan_dir_gates() {
        let f = base_fs().expect("f");
        assert_eq!(plan(&f, b"/nope", &Opt::new()).err(), Some(IN_ENODIR));
        assert_eq!(plan(&f, b"/bin/busybox", &Opt::new()).err(), Some(IN_ENOTDIR));
        assert_eq!(plan(&Fs::new(), b"/bin", &Opt::new()).err(), Some(IN_ENODIR));
        assert_eq!(plan(&f, &[0x01], &Opt::new()).err(), Some(IN_EBYTE));
    }

    #[test]
    fn conflicts_are_named() {
        assert_eq!(conflict_with(Entry::file(b"/bin/ls")), Ok(1));
        assert_eq!(conflict_with(Entry::link(b"/bin/ls", b"/bin/sh")), Ok(1));
        assert_eq!(conflict_with(Entry::dir(b"/bin/ls")), Ok(1));
        let f = with_file(b"/bin/ls");
        assert_eq!(
            plan(&f, b"/bin", &Opt::new()).map(|p| p.first_conflict()),
            Ok(Some(IN_EEXIST))
        );
        assert_eq!(plan_clean(&f, b"/bin", &Opt::new()), Err(IN_EEXIST));
    }

    #[test]
    fn install_then_idempotent() {
        let f = installed_fs().expect("f");
        assert_eq!(f.link_count(), APPLETS);
        let p = plan(&f, b"/bin", &Opt::new()).expect("p");
        assert_eq!(p.kept(), APPLETS);
        assert_eq!(p.created(), 0);
        assert!(p.clean());
    }

    #[test]
    fn apply_counts() {
        let mut f = base_fs().expect("f");
        let p = plan(&f, b"/bin", &Opt::new()).expect("p");
        let st = p.apply_to(&mut f).expect("st");
        assert_eq!(st.created, APPLETS);
        assert_eq!(st.text().as_str(), "created=40 kept=0 replaced=0");
        let p2 = plan(&f, b"/bin", &Opt::new()).expect("p2");
        let st2 = p2.apply_to(&mut f).expect("st2");
        assert_eq!(st2.kept, APPLETS);
        assert_eq!(st2.created, 0);
    }

    #[test]
    fn replace_file() {
        let mut f = with_file(b"/bin/ls");
        let opt = Opt { replace: true, relative: false, dry: false };
        let p = plan(&f, b"/bin", &opt).expect("p");
        assert_eq!(p.replaced(), 1);
        assert_eq!(p.conflicts(), 0);
        let st = p.apply_to(&mut f).expect("st");
        assert_eq!(st.replaced, 1);
        assert_eq!(f.kind_at(b"/bin/ls"), Some(Kind::Link));
    }

    #[test]
    fn relative_target_mismatch() {
        let f = base_fs().expect("f");
        let opt = Opt { replace: false, relative: true, dry: false };
        let p = plan(&f, b"/bin", &opt).expect("p");
        assert_eq!(p.target().text().as_str(), "busybox");
        let mut fs = f;
        p.apply_to(&mut fs).expect("apply");
        let abs = plan(&fs, b"/bin", &Opt::new()).expect("abs");
        assert_eq!(abs.conflicts(), APPLETS);
        assert_eq!(abs.first_conflict(), Some(IN_EWRONG));
    }

    #[test]
    fn options_require_install() {
        assert_eq!(parse_argv(&[b"--install"]).map(|o| o.replace), Ok(false));
        assert_eq!(parse_argv(&[b"--install", b"-f"]).map(|o| o.replace), Ok(true));
        assert_eq!(parse_argv(&[b"-f"]).err(), Some(IN_EOPT));
        assert_eq!(parse_argv(&[b"--install", b"-z"]).err(), Some(IN_EOPT));
        assert_eq!(parse_argv(&[b"--install", b"x"]).err(), Some(IN_EOPT));
    }

    #[test]
    fn entry_text() {
        let e = Entry::link(b"/bin/ls", b"/bin/busybox").expect("e");
        assert_eq!(e.text().as_str(), "l /bin/ls -> /bin/busybox");
        let d = Entry::dir(b"/bin").expect("d");
        assert_eq!(d.text().as_str(), "d /bin");
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.sample(0).map(|s| s.value), Some(2));
        assert_eq!(l.sample(6).map(|s| s.value), Some(12));
        assert_eq!(l.sample(8).map(|s| s.code), Some(IN_ENODIR));
        assert_eq!(l.sample(13).map(|s| s.code), Some(IN_EBYTE));
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = l.freeze().expect("freeze");
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(IN_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(IN_ESLOT));
        assert_eq!(l.rejects(), 2);
        assert!(l.is_frozen());
    }

    #[test]
    fn injections_all_true() {
        assert_eq!(inject_four(), [true, true, true, true]);
    }

    #[test]
    fn check_count_is_thirteen() {
        assert_eq!(run_unx11584_checks().len(), 13);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11584_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}

