//! UNX-F9231 EBADF vs ENOENT 判定次序协议（先 fd 后路径） · 0x7A740段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9231 逐字）：
//!   ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9231-J1 判序协议判据过，坏 fd
//!   优先返回 EBADF 逐格检出，判序与 Linux 逐例一致格过；③ 完成时 API/软件
//!   写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 判序协议固定七段：标志 → 指针 → 名字 → fd → 目录性 → 查找 → 对象。
//!      任一段失败立即终止，后续段**一个都不访问**——trace 里出现查找段而
//!      结果却是 EBADF，就说明实现偷偷「先查路径后验 fd」，直接判红。
//!   ② 「先 fd 后路径」只对相对路径成立。绝对路径在 path_init 阶段钉死
//!      root，dirfd 整个被跳过，所以「坏 fd + 绝对路径」返回 ENOENT 而不是
//!      EBADF——这是本卡最大的反例格，必须逐格显性对账。
//!   ③ 空路径且未置 AT_EMPTY_PATH：getname_flags 在名字段就回 ENOENT，早于
//!      fd；置了标志后 fd 成为唯一对象来源，坏 fd 才轮到 EBADF。两面对照才
//!      能证明「名字段早于 fd 段、fd 段早于查找段」这条偏序。
//!   ④ 合法 fd 但不是目录：目录性段回 ENOTDIR，仍早于查找。目标文件根本不
//!      存在也得先报 ENOTDIR，不允许把 ENOTDIR 降级成 ENOENT。
//!   ⑤ 标志校验（EINVAL）排在指针与 fd 之前；错误用户指针（EFAULT）排在
//!      名字之前。这两段都是「遮罩段」：更靠后的坏 fd 不得越过它们抢答。
//!   ⑥ 注入器做四族 × 六 fd 类 × 五路径 = 120 发全交叉，账本按错误码分桶
//!      严格守恒；另记 lookup_after_fd / backtrack / abs_leak / priority_miss
//!      四类违例，全零才算过。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x7A740;

pub const ENOENT: i32 = 2;
pub const EBADF: i32 = 9;
pub const EFAULT: i32 = 14;
pub const ENOTDIR: i32 = 20;
pub const EISDIR: i32 = 21;
pub const EINVAL: i32 = 22;
pub const EMFILE: i32 = 24;

pub const AT_FDCWD: i32 = -100;
pub const AT_SYMLINK_NOFOLLOW: u32 = 0x100;
pub const AT_REMOVEDIR: u32 = 0x200;
pub const AT_EMPTY_PATH: u32 = 0x1000;

pub const FIRST_FD: i32 = 3;
pub const MAX_FDS: usize = 6;

/// 诊断码闭集（值即 Linux errno，Ok 记 0）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OrderCode {
    Ok,
    Ebadf,
    Enoent,
    Enotdir,
    Eisdir,
    Einval,
    Efault,
    Eloop,
}

impl OrderCode {
    pub const ALL: [OrderCode; 8] = [
        OrderCode::Ok,
        OrderCode::Ebadf,
        OrderCode::Enoent,
        OrderCode::Enotdir,
        OrderCode::Eisdir,
        OrderCode::Einval,
        OrderCode::Efault,
        OrderCode::Eloop,
    ];

    pub fn val(self) -> i32 {
        match self {
            OrderCode::Ok => 0,
            OrderCode::Ebadf => EBADF,
            OrderCode::Enoent => ENOENT,
            OrderCode::Enotdir => ENOTDIR,
            OrderCode::Eisdir => EISDIR,
            OrderCode::Einval => EINVAL,
            OrderCode::Efault => EFAULT,
            OrderCode::Eloop => 40,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            OrderCode::Ok => "OK",
            OrderCode::Ebadf => "EBADF",
            OrderCode::Enoent => "ENOENT",
            OrderCode::Enotdir => "ENOTDIR",
            OrderCode::Eisdir => "EISDIR",
            OrderCode::Einval => "EINVAL",
            OrderCode::Efault => "EFAULT",
            OrderCode::Eloop => "ELOOP",
        }
    }

    /// 中文处置建议（闭集内每条都必须有人可读的下一步）。
    pub fn detail(self) -> &'static str {
        match self {
            OrderCode::Ok => "判序未触发错误，返回对象结果",
            OrderCode::Ebadf => "先验 fd 槽位与代际，坏句柄不得进入路径遍历",
            OrderCode::Enoent => "fd 面通过后路径缺失，按解析错误面上报",
            OrderCode::Enotdir => "目录性段拦截：前缀或 dirfd 不是目录",
            OrderCode::Eisdir => "对象段拦截：目录不接受该操作形态",
            OrderCode::Einval => "标志/参数段拦截：先于 fd 与路径返回",
            OrderCode::Efault => "用户指针段拦截：拷贝失败先于名字解析",
            OrderCode::Eloop => "符号链接环超限，查找段终止",
        }
    }
}

/// 把 errno 折回闭集（未知错误码按 EINVAL 归属，绝不静默吞）。
pub fn code_of(errno: i32) -> OrderCode {
    match errno {
        0 => OrderCode::Ok,
        EBADF => OrderCode::Ebadf,
        ENOENT => OrderCode::Enoent,
        ENOTDIR => OrderCode::Enotdir,
        EISDIR => OrderCode::Eisdir,
        EINVAL => OrderCode::Einval,
        EFAULT => OrderCode::Efault,
        40 => OrderCode::Eloop,
        _ => OrderCode::Einval,
    }
}

/// 判序七段。打印序即执行序，idx 严格递增。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Flags,
    Pointer,
    Name,
    Fd,
    DirKind,
    Lookup,
    Object,
}

impl Stage {
    pub const ALL: [Stage; 7] = [
        Stage::Flags,
        Stage::Pointer,
        Stage::Name,
        Stage::Fd,
        Stage::DirKind,
        Stage::Lookup,
        Stage::Object,
    ];

    pub fn idx(self) -> usize {
        match self {
            Stage::Flags => 0,
            Stage::Pointer => 1,
            Stage::Name => 2,
            Stage::Fd => 3,
            Stage::DirKind => 4,
            Stage::Lookup => 5,
            Stage::Object => 6,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Stage::Flags => "标志段",
            Stage::Pointer => "指针段",
            Stage::Name => "名字段",
            Stage::Fd => "fd段",
            Stage::DirKind => "目录性段",
            Stage::Lookup => "查找段",
            Stage::Object => "对象段",
        }
    }
}

/// 协议条款：每段问什么、失败归哪个码。
#[derive(Clone, Copy)]
pub struct Rule {
    pub stage: Stage,
    pub question: &'static str,
    pub on_fail: OrderCode,
}

pub const PROTOCOL: [Rule; 7] = [
    Rule { stage: Stage::Flags, question: "标志位是否在本族合法集内", on_fail: OrderCode::Einval },
    Rule { stage: Stage::Pointer, question: "用户指针能否安全拷贝", on_fail: OrderCode::Efault },
    Rule { stage: Stage::Name, question: "路径名长度与空串是否合规", on_fail: OrderCode::Enoent },
    Rule { stage: Stage::Fd, question: "dirfd 槽位是否存在且代际有效", on_fail: OrderCode::Ebadf },
    Rule { stage: Stage::DirKind, question: "dirfd 指向的是否目录", on_fail: OrderCode::Enotdir },
    Rule { stage: Stage::Lookup, question: "路径逐组件查找是否命中", on_fail: OrderCode::Enoent },
    Rule { stage: Stage::Object, question: "对象类型是否接受本操作", on_fail: OrderCode::Eisdir },
];

/// 四族系统调用（本卡只关心「fd 面与路径面谁先说话」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    Openat,
    Fstatat,
    Unlinkat,
    Readlinkat,
}

pub const FAMILIES: [Family; 4] = [
    Family::Openat,
    Family::Fstatat,
    Family::Unlinkat,
    Family::Readlinkat,
];

impl Family {
    pub fn label(self) -> &'static str {
        match self {
            Family::Openat => "openat",
            Family::Fstatat => "fstatat",
            Family::Unlinkat => "unlinkat",
            Family::Readlinkat => "readlinkat",
        }
    }

    /// 合法 AT_* / O_* 位集。openat 的掩码只留 O_WRONLY|O_RDWR|O_CREAT|O_DIRECTORY。
    pub fn flag_mask(self) -> u32 {
        match self {
            Family::Openat => 0x0001_0043,
            Family::Fstatat => AT_SYMLINK_NOFOLLOW | AT_EMPTY_PATH,
            Family::Unlinkat => AT_REMOVEDIR,
            Family::Readlinkat => AT_EMPTY_PATH,
        }
    }

    pub fn flags_ok(self, flags: u32) -> bool {
        flags & !self.flag_mask() == 0
    }

    /// 空路径 + 该标志才允许把 fd 当对象。
    pub fn empty_allowed(self) -> bool {
        match self {
            Family::Fstatat => true,
            Family::Readlinkat => true,
            Family::Openat => false,
            Family::Unlinkat => false,
        }
    }

    /// 末段符号链接是否跟随。
    pub fn follows(self, flags: u32) -> bool {
        match self {
            Family::Openat => true,
            Family::Fstatat => flags & AT_SYMLINK_NOFOLLOW == 0,
            Family::Unlinkat => false,
            Family::Readlinkat => false,
        }
    }
}

/// fd 类（注入面枚举，覆盖合法/坏值全部形态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FdClass {
    Cwd,
    Dir,
    File,
    Closed,
    Beyond,
    Negative,
}

impl FdClass {
    pub const ALL: [FdClass; 6] = [
        FdClass::Cwd,
        FdClass::Dir,
        FdClass::File,
        FdClass::Closed,
        FdClass::Beyond,
        FdClass::Negative,
    ];

    pub fn fd(self) -> i32 {
        match self {
            FdClass::Cwd => AT_FDCWD,
            FdClass::Dir => FIRST_FD,
            FdClass::File => FIRST_FD + 1,
            FdClass::Closed => FIRST_FD + 2,
            FdClass::Beyond => FIRST_FD + MAX_FDS as i32 + 1,
            FdClass::Negative => -1,
        }
    }

    pub fn is_bad(self) -> bool {
        match self {
            FdClass::Closed | FdClass::Beyond | FdClass::Negative => true,
            FdClass::Cwd | FdClass::Dir | FdClass::File => false,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FdClass::Cwd => "AT_FDCWD",
            FdClass::Dir => "目录fd",
            FdClass::File => "文件fd",
            FdClass::Closed => "已关fd",
            FdClass::Beyond => "越界fd",
            FdClass::Negative => "负值fd",
        }
    }
}

/// 执行轨迹：只记已访问段，用于证明「失败后不再前进」。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Trace {
    buf: [u8; 7],
    len: usize,
}

impl Trace {
    pub fn new() -> Trace {
        Trace { buf: [0; 7], len: 0 }
    }

    pub fn push(&mut self, stage: Stage) {
        if self.len < self.buf.len() {
            if let Some(slot) = self.buf.get_mut(self.len) {
                *slot = stage.idx() as u8;
                self.len += 1;
            }
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn get(&self, i: usize) -> Option<Stage> {
        let raw = *self.buf.get(i)?;
        match raw {
            0 => Some(Stage::Flags),
            1 => Some(Stage::Pointer),
            2 => Some(Stage::Name),
            3 => Some(Stage::Fd),
            4 => Some(Stage::DirKind),
            5 => Some(Stage::Lookup),
            6 => Some(Stage::Object),
            _ => None,
        }
    }

    pub fn has(&self, stage: Stage) -> bool {
        let want = stage.idx() as u8;
        let mut i = 0usize;
        while i < self.len {
            if let Some(raw) = self.buf.get(i) {
                if *raw == want {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 轨迹必须严格递增：任何回退都意味着判序被重排。
    pub fn monotonic(&self) -> bool {
        let mut prev: i32 = -1;
        let mut i = 0usize;
        while i < self.len {
            let cur = match self.buf.get(i) {
                Some(raw) => *raw as i32,
                None => return false,
            };
            if cur <= prev {
                return false;
            }
            prev = cur;
            i += 1;
        }
        true
    }
}

/// fd 槽位状态机：Free → Open(kind) → Closed，复用时代际 +1。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObjKind {
    Free,
    Dir,
    File,
}

#[derive(Clone, Copy)]
struct Slot {
    kind: ObjKind,
    gen: u16,
}

pub struct FdTable {
    slots: [Slot; MAX_FDS],
    open: usize,
}

impl FdTable {
    pub const fn new() -> FdTable {
        FdTable {
            slots: [Slot { kind: ObjKind::Free, gen: 0 }; MAX_FDS],
            open: 0,
        }
    }

    fn index(&self, fd: i32) -> Option<usize> {
        if fd < FIRST_FD {
            return None;
        }
        let off = (fd - FIRST_FD) as usize;
        if off >= MAX_FDS {
            return None;
        }
        Some(off)
    }

    pub fn open(&mut self, kind: ObjKind) -> Result<(i32, u16), i32> {
        if kind == ObjKind::Free {
            return Err(EINVAL);
        }
        let mut i = 0usize;
        while i < MAX_FDS {
            let free = match self.slots.get(i) {
                Some(slot) => slot.kind == ObjKind::Free,
                None => false,
            };
            if free {
                let (fd, gen) = match self.slots.get_mut(i) {
                    Some(slot) => {
                        slot.kind = kind;
                        (FIRST_FD + i as i32, slot.gen)
                    }
                    None => return Err(EMFILE),
                };
                self.open = self.open.saturating_add(1);
                return Ok((fd, gen));
            }
            i += 1;
        }
        Err(EMFILE)
    }

    pub fn close(&mut self, fd: i32) -> Result<u16, i32> {
        let idx = match self.index(fd) {
            Some(i) => i,
            None => return Err(EBADF),
        };
        let gen = match self.slots.get_mut(idx) {
            Some(slot) => {
                if slot.kind == ObjKind::Free {
                    return Err(EBADF);
                }
                slot.kind = ObjKind::Free;
                slot.gen = slot.gen.wrapping_add(1);
                slot.gen
            }
            None => return Err(EBADF),
        };
        self.open = self.open.saturating_sub(1);
        Ok(gen)
    }

    pub fn describe(&self, fd: i32) -> Option<(ObjKind, u16)> {
        let idx = self.index(fd)?;
        let slot = self.slots.get(idx)?;
        if slot.kind == ObjKind::Free {
            None
        } else {
            Some((slot.kind, slot.gen))
        }
    }

    pub fn describe_gen(&self, fd: i32, gen: u16) -> Result<ObjKind, i32> {
        match self.describe(fd) {
            Some((kind, g)) if g == gen => Ok(kind),
            _ => Err(EBADF),
        }
    }

    pub fn open_count(&self) -> usize {
        self.open
    }
}

/// 命名空间条目（扁平键，避免堆分配；根键为空串）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Dir,
    File,
    Symlink,
}

#[derive(Clone, Copy)]
pub struct Entry {
    pub key: &'static str,
    pub kind: Kind,
    pub target: &'static str,
}

pub const ENTRIES: [Entry; 6] = [
    Entry { key: "", kind: Kind::Dir, target: "" },
    Entry { key: "dir", kind: Kind::Dir, target: "" },
    Entry { key: "dir/inner", kind: Kind::File, target: "" },
    Entry { key: "file", kind: Kind::File, target: "" },
    Entry { key: "link", kind: Kind::Symlink, target: "dir" },
    Entry { key: "dangling", kind: Kind::Symlink, target: "ghost" },
];

pub fn entry(key: &str) -> Option<Entry> {
    ENTRIES.iter().copied().find(|e| e.key == key)
}

/// 单次查找结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Walk {
    Found(Kind),
    Link(&'static str),
    Missing,
    NonDir,
}

/// 逐组件走命名空间：前缀是普通文件即 NonDir；末段缺失即 Missing。
pub fn walk(text: &str) -> Walk {
    let trimmed = text.trim_end_matches('/');
    let key = if trimmed.starts_with('/') {
        trimmed.get(1..).unwrap_or("")
    } else {
        trimmed
    };
    if key.is_empty() {
        return Walk::Found(Kind::Dir);
    }
    if let Some(e) = entry(key) {
        return match e.kind {
            Kind::Symlink => Walk::Link(e.target),
            other => Walk::Found(other),
        };
    }
    let mut probe = key;
    loop {
        match probe.rfind('/') {
            Some(cut) => {
                let parent = probe.get(..cut).unwrap_or("");
                if let Some(e) = entry(parent) {
                    match e.kind {
                        Kind::File => return Walk::NonDir,
                        Kind::Dir => return Walk::Missing,
                        Kind::Symlink => return Walk::Missing,
                    }
                }
                if parent.is_empty() {
                    break;
                }
                probe = parent;
            }
            None => break,
        }
    }
    Walk::Missing
}

/// 路径类（用于矩阵/对账分类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathClass {
    AbsPresent,
    AbsMissing,
    RelPresent,
    RelMissing,
    NonDirPrefix,
    Empty,
}

pub fn class_of(text: &str) -> PathClass {
    if text.is_empty() {
        return PathClass::Empty;
    }
    let abs = text.starts_with('/');
    match walk(text) {
        Walk::Found(_) | Walk::Link(_) => {
            if abs {
                PathClass::AbsPresent
            } else {
                PathClass::RelPresent
            }
        }
        Walk::Missing => {
            if abs {
                PathClass::AbsMissing
            } else {
                PathClass::RelMissing
            }
        }
        Walk::NonDir => PathClass::NonDirPrefix,
    }
}

/// 一次判定输入。
#[derive(Clone, Copy)]
pub struct Probe {
    pub fam: Family,
    pub dirfd: i32,
    pub path: &'static str,
    pub flags: u32,
    pub bad_ptr: bool,
}

pub fn mk(fam: Family, dirfd: i32, path: &'static str, flags: u32) -> Probe {
    Probe { fam, dirfd, path, flags, bad_ptr: false }
}

/// 判定输出：错误码 + 终止段 + 轨迹。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Verdict {
    pub code: i32,
    pub stage: Stage,
    pub trace: Trace,
}

impl Verdict {
    fn ok(trace: Trace) -> Verdict {
        Verdict { code: 0, stage: Stage::Object, trace }
    }

    fn fail(errno: i32, stage: Stage, trace: Trace) -> Verdict {
        Verdict { code: errno, stage, trace }
    }
}

/// 判序协议本体：七段固定序，失败即停。
pub fn evaluate(t: &FdTable, p: Probe) -> Verdict {
    let mut trace = Trace::new();

    trace.push(Stage::Flags);
    if !p.fam.flags_ok(p.flags) {
        return Verdict::fail(EINVAL, Stage::Flags, trace);
    }

    trace.push(Stage::Pointer);
    if p.bad_ptr {
        return Verdict::fail(EFAULT, Stage::Pointer, trace);
    }

    trace.push(Stage::Name);
    let empty = p.path.is_empty();
    let empty_ok = empty && p.fam.empty_allowed() && p.flags & AT_EMPTY_PATH != 0;
    if empty && !empty_ok {
        return Verdict::fail(ENOENT, Stage::Name, trace);
    }

    let absolute = p.path.starts_with('/');
    if !empty && absolute {
        // 绝对路径：path_init 钉 root，dirfd 完全不参与，fd/目录性两段跳过。
    } else {
        trace.push(Stage::Fd);
        let dir_kind = if p.dirfd == AT_FDCWD {
            Some(ObjKind::Dir)
        } else if p.dirfd < 0 {
            None
        } else {
            match t.describe(p.dirfd) {
                Some(pair) => Some(pair.0),
                None => None,
            }
        };
        let kind = match dir_kind {
            Some(k) => k,
            None => return Verdict::fail(EBADF, Stage::Fd, trace),
        };
        trace.push(Stage::DirKind);
        if kind != ObjKind::Dir {
            return Verdict::fail(ENOTDIR, Stage::DirKind, trace);
        }
    }

    if empty {
        // AT_EMPTY_PATH：fd 自身就是对象，不再进入查找段。
        trace.push(Stage::Object);
        return Verdict::ok(trace);
    }

    trace.push(Stage::Lookup);
    let followed = match walk(p.path) {
        Walk::Found(kind) => Ok(kind),
        Walk::Link(target) => {
            if p.fam.follows(p.flags) {
                match entry(target) {
                    Some(e) if e.kind == Kind::Dir => Ok(Kind::Dir),
                    Some(e) if e.kind == Kind::File => Ok(Kind::File),
                    _ => Err(ENOENT),
                }
            } else {
                Ok(Kind::Symlink)
            }
        }
        Walk::Missing => Err(ENOENT),
        Walk::NonDir => Err(ENOTDIR),
    };
    let kind = match followed {
        Ok(k) => k,
        Err(errno) => return Verdict::fail(errno, Stage::Lookup, trace),
    };

    trace.push(Stage::Object);
    match p.fam {
        Family::Openat => {
            if kind == Kind::Dir && p.flags & 0x3 != 0 {
                return Verdict::fail(EISDIR, Stage::Object, trace);
            }
        }
        Family::Unlinkat => {
            if kind == Kind::Dir && p.flags & AT_REMOVEDIR == 0 {
                return Verdict::fail(EISDIR, Stage::Object, trace);
            }
        }
        Family::Readlinkat => {
            if kind != Kind::Symlink {
                return Verdict::fail(EINVAL, Stage::Object, trace);
            }
        }
        Family::Fstatat => {}
    }
    Verdict::ok(trace)
}

/// 样例 fd 表：3=目录、4=文件、5=已关。
pub fn sample_table() -> FdTable {
    let mut t = FdTable::new();
    let _ = t.open(ObjKind::Dir);
    let _ = t.open(ObjKind::File);
    let _ = t.open(ObjKind::Dir);
    let _ = t.close(FIRST_FD + 2);
    t
}

/// 判序对账行（人工逐例写法，不按实现反推）。
#[derive(Clone, Copy)]
pub struct Precedence {
    pub fam: Family,
    pub fd: FdClass,
    pub path: &'static str,
    pub flags: u32,
    pub stage: Stage,
    pub code: OrderCode,
}

pub const PRECEDENCE: [Precedence; 24] = [
    Precedence { fam: Family::Openat, fd: FdClass::Dir, path: "dir/inner", flags: 0, stage: Stage::Object, code: OrderCode::Ok },
    Precedence { fam: Family::Openat, fd: FdClass::File, path: "dir/miss", flags: 0, stage: Stage::DirKind, code: OrderCode::Enotdir },
    Precedence { fam: Family::Openat, fd: FdClass::Closed, path: "dir/miss", flags: 0, stage: Stage::Fd, code: OrderCode::Ebadf },
    Precedence { fam: Family::Openat, fd: FdClass::Beyond, path: "dir/miss", flags: 0, stage: Stage::Fd, code: OrderCode::Ebadf },
    Precedence { fam: Family::Openat, fd: FdClass::Negative, path: "dir/miss", flags: 0, stage: Stage::Fd, code: OrderCode::Ebadf },
    Precedence { fam: Family::Openat, fd: FdClass::Cwd, path: "/dir/miss", flags: 0, stage: Stage::Lookup, code: OrderCode::Enoent },

    Precedence { fam: Family::Fstatat, fd: FdClass::Dir, path: "dir/inner", flags: 0, stage: Stage::Object, code: OrderCode::Ok },
    Precedence { fam: Family::Fstatat, fd: FdClass::File, path: "dir/inner", flags: 0, stage: Stage::DirKind, code: OrderCode::Enotdir },
    Precedence { fam: Family::Fstatat, fd: FdClass::Closed, path: "dir/miss", flags: 0, stage: Stage::Fd, code: OrderCode::Ebadf },
    Precedence { fam: Family::Fstatat, fd: FdClass::Beyond, path: "dir/inner", flags: 0, stage: Stage::Fd, code: OrderCode::Ebadf },
    Precedence { fam: Family::Fstatat, fd: FdClass::Negative, path: "dir/inner", flags: 0, stage: Stage::Fd, code: OrderCode::Ebadf },
    Precedence { fam: Family::Fstatat, fd: FdClass::Cwd, path: "dir/miss", flags: 0, stage: Stage::Lookup, code: OrderCode::Enoent },

    Precedence { fam: Family::Unlinkat, fd: FdClass::Dir, path: "dir/inner", flags: 0, stage: Stage::Object, code: OrderCode::Ok },
    Precedence { fam: Family::Unlinkat, fd: FdClass::Dir, path: "dir", flags: 0, stage: Stage::Object, code: OrderCode::Eisdir },
    Precedence { fam: Family::Unlinkat, fd: FdClass::File, path: "dir/inner", flags: 0, stage: Stage::DirKind, code: OrderCode::Enotdir },
    Precedence { fam: Family::Unlinkat, fd: FdClass::Closed, path: "dir/inner", flags: 0, stage: Stage::Fd, code: OrderCode::Ebadf },
    Precedence { fam: Family::Unlinkat, fd: FdClass::Negative, path: "dir/inner", flags: 0, stage: Stage::Fd, code: OrderCode::Ebadf },
    Precedence { fam: Family::Unlinkat, fd: FdClass::Cwd, path: "link", flags: 0, stage: Stage::Object, code: OrderCode::Ok },

    Precedence { fam: Family::Readlinkat, fd: FdClass::Dir, path: "link", flags: 0, stage: Stage::Object, code: OrderCode::Ok },
    Precedence { fam: Family::Readlinkat, fd: FdClass::Dir, path: "dir/inner", flags: 0, stage: Stage::Object, code: OrderCode::Einval },
    Precedence { fam: Family::Readlinkat, fd: FdClass::File, path: "link", flags: 0, stage: Stage::DirKind, code: OrderCode::Enotdir },
    Precedence { fam: Family::Readlinkat, fd: FdClass::Closed, path: "link", flags: 0, stage: Stage::Fd, code: OrderCode::Ebadf },
    Precedence { fam: Family::Readlinkat, fd: FdClass::Negative, path: "link", flags: 0, stage: Stage::Fd, code: OrderCode::Ebadf },
    Precedence { fam: Family::Readlinkat, fd: FdClass::Cwd, path: "dangling", flags: 0, stage: Stage::Object, code: OrderCode::Ok },
];

/// 网格核算结果。
#[derive(Clone, Copy)]
pub struct GridTally {
    pub cells: usize,
    pub mismatches: usize,
    pub ok: usize,
    pub ebadf: usize,
    pub enoent: usize,
    pub enotdir: usize,
    pub eisdir: usize,
    pub einval: usize,
}

pub fn run_grid() -> GridTally {
    let t = sample_table();
    let mut g = GridTally {
        cells: 0,
        mismatches: 0,
        ok: 0,
        ebadf: 0,
        enoent: 0,
        enotdir: 0,
        eisdir: 0,
        einval: 0,
    };
    let mut i = 0usize;
    while i < PRECEDENCE.len() {
        if let Some(row) = PRECEDENCE.get(i) {
            let p = mk(row.fam, row.fd.fd(), row.path, row.flags);
            let v = evaluate(&t, p);
            g.cells += 1;
            if v.code != row.code.val() || v.stage != row.stage {
                g.mismatches += 1;
            }
            match v.code {
                0 => g.ok += 1,
                EBADF => g.ebadf += 1,
                ENOENT => g.enoent += 1,
                ENOTDIR => g.enotdir += 1,
                EISDIR => g.eisdir += 1,
                EINVAL => g.einval += 1,
                _ => {}
            }
        }
        i += 1;
    }
    g
}

/// 注入账本：按错误码分桶守恒，另记四类判序违例。
#[derive(Clone, Copy)]
pub struct OrderLedger {
    pub shots: usize,
    pub ok: usize,
    pub ebadf: usize,
    pub enoent: usize,
    pub enotdir: usize,
    pub einval: usize,
    pub other: usize,
    pub lookup_after_fd: usize,
    pub backtrack: usize,
    pub abs_leak: usize,
    pub priority_miss: usize,
}

impl OrderLedger {
    pub fn conserved(&self) -> bool {
        let sum = self.ok
            + self.ebadf
            + self.enoent
            + self.enotdir
            + self.einval
            + self.other;
        self.shots > 0
            && sum == self.shots
            && self.lookup_after_fd == 0
            && self.backtrack == 0
            && self.abs_leak == 0
            && self.priority_miss == 0
    }
}

pub const INJECT_PATHS: [&str; 5] = ["dir/inner", "dir/miss", "/dir/miss", "", "file/miss"];

/// 120 发全交叉注入：4 族 × 6 fd 类 × 5 路径。
pub fn run_injection() -> OrderLedger {
    let t = sample_table();
    let mut l = OrderLedger {
        shots: 0,
        ok: 0,
        ebadf: 0,
        enoent: 0,
        enotdir: 0,
        einval: 0,
        other: 0,
        lookup_after_fd: 0,
        backtrack: 0,
        abs_leak: 0,
        priority_miss: 0,
    };
    for fam in FAMILIES.iter().copied() {
        for cls in FdClass::ALL.iter().copied() {
            for path in INJECT_PATHS.iter().copied() {
                let v = evaluate(&t, mk(fam, cls.fd(), path, 0));
                l.shots += 1;
                match v.code {
                    0 => l.ok += 1,
                    EBADF => l.ebadf += 1,
                    ENOENT => l.enoent += 1,
                    ENOTDIR => l.enotdir += 1,
                    EINVAL => l.einval += 1,
                    _ => l.other += 1,
                }
                if !v.trace.monotonic() {
                    l.backtrack += 1;
                }
                if v.code == EBADF && v.trace.has(Stage::Lookup) {
                    l.lookup_after_fd += 1;
                }
                if v.code == EBADF && (path.is_empty() || path.starts_with('/')) {
                    l.abs_leak += 1;
                }
                if cls.is_bad() && !path.is_empty() && !path.starts_with('/') && v.code != EBADF
                {
                    l.priority_miss += 1;
                }
            }
        }
    }
    l
}

/// Linux 逐例对账表（含空路径与标志段的反例格）。
#[derive(Clone, Copy)]
pub struct Recon {
    pub fam: Family,
    pub fd: FdClass,
    pub path: &'static str,
    pub flags: u32,
    pub stage: Stage,
    pub code: OrderCode,
}

pub const RECON: [Recon; 16] = [
    Recon { fam: Family::Openat, fd: FdClass::Dir, path: "dir/inner", flags: 0, stage: Stage::Object, code: OrderCode::Ok },
    Recon { fam: Family::Openat, fd: FdClass::File, path: "dir/inner", flags: 0, stage: Stage::DirKind, code: OrderCode::Enotdir },
    Recon { fam: Family::Openat, fd: FdClass::Closed, path: "dir/miss", flags: 0, stage: Stage::Fd, code: OrderCode::Ebadf },
    Recon { fam: Family::Openat, fd: FdClass::Beyond, path: "dir/inner", flags: 0, stage: Stage::Fd, code: OrderCode::Ebadf },
    Recon { fam: Family::Openat, fd: FdClass::Negative, path: "/dir/miss", flags: 0, stage: Stage::Lookup, code: OrderCode::Enoent },
    Recon { fam: Family::Openat, fd: FdClass::Dir, path: "", flags: 0, stage: Stage::Name, code: OrderCode::Enoent },
    Recon { fam: Family::Fstatat, fd: FdClass::Dir, path: "link", flags: 0, stage: Stage::Object, code: OrderCode::Ok },
    Recon { fam: Family::Fstatat, fd: FdClass::Dir, path: "dangling", flags: 0, stage: Stage::Lookup, code: OrderCode::Enoent },
    Recon { fam: Family::Fstatat, fd: FdClass::Dir, path: "dangling", flags: AT_SYMLINK_NOFOLLOW, stage: Stage::Object, code: OrderCode::Ok },
    Recon { fam: Family::Fstatat, fd: FdClass::Negative, path: "", flags: AT_EMPTY_PATH, stage: Stage::Fd, code: OrderCode::Ebadf },
    Recon { fam: Family::Fstatat, fd: FdClass::Dir, path: "", flags: AT_EMPTY_PATH, stage: Stage::Object, code: OrderCode::Ok },
    Recon { fam: Family::Unlinkat, fd: FdClass::Dir, path: "dir", flags: 0, stage: Stage::Object, code: OrderCode::Eisdir },
    Recon { fam: Family::Unlinkat, fd: FdClass::Dir, path: "dir", flags: AT_REMOVEDIR, stage: Stage::Object, code: OrderCode::Ok },
    Recon { fam: Family::Unlinkat, fd: FdClass::Dir, path: "dir", flags: AT_SYMLINK_NOFOLLOW, stage: Stage::Flags, code: OrderCode::Einval },
    Recon { fam: Family::Readlinkat, fd: FdClass::Dir, path: "link", flags: 0, stage: Stage::Object, code: OrderCode::Ok },
    Recon { fam: Family::Readlinkat, fd: FdClass::Dir, path: "file", flags: 0, stage: Stage::Object, code: OrderCode::Einval },
];

/// FNV-1a 32 位（无浮点、无查表堆）。
pub fn fnv1a(mut h: u32, bytes: &[u8]) -> u32 {
    for b in bytes.iter().copied() {
        h ^= b as u32;
        h = h.wrapping_mul(16_777_619);
    }
    h
}

/// 语料指纹：把对账表与账本分桶折进一个 u32，双跑必须相等。
pub fn corpus_fingerprint() -> u32 {
    let mut h = 0x811C_9DC5u32;
    let mut i = 0usize;
    while i < PRECEDENCE.len() {
        if let Some(row) = PRECEDENCE.get(i) {
            h = fnv1a(h, row.path.as_bytes());
            h = fnv1a(h, &[row.code.val() as u8, row.stage.idx() as u8]);
        }
        i += 1;
    }
    let l = run_injection();
    let buckets = [
        l.shots as u8,
        l.ebadf as u8,
        l.enoent as u8,
        l.enotdir as u8,
        l.ok as u8,
    ];
    h = fnv1a(h, &buckets);
    h
}

/// 本卡自检入口（≥12 条，动态明细 ≤8 条）。
pub fn run_unx9231_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9231");

    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add(
        "段位守卫：0x40 对齐且落在 0x7A000..=0x7D1C0",
        seg_ok,
        "段基址必须唯一；错位即拒绝注册",
    );

    let mut code_ok = true;
    let mut i = 0usize;
    while i < OrderCode::ALL.len() {
        if let Some(c) = OrderCode::ALL.get(i) {
            if c.detail().is_empty() || code_of(c.val()) != *c {
                code_ok = false;
            }
        }
        i += 1;
    }
    code_ok = code_ok && OrderCode::Ok.val() == 0 && code_of(EBADF) == OrderCode::Ebadf;
    s.add(
        "诊断码闭集：8 码值唯一、detail 非空、errno 往返一致",
        code_ok,
        "闭集外的错误码一律折回 EINVAL，不静默吞",
    );

    let mut proto_ok = PROTOCOL.len() == 7;
    let mut prev: i32 = -1;
    let mut j = 0usize;
    while j < PROTOCOL.len() {
        if let Some(rule) = PROTOCOL.get(j) {
            let idx = rule.stage.idx() as i32;
            if idx <= prev || rule.question.is_empty() || rule.on_fail == OrderCode::Ok {
                proto_ok = false;
            }
            prev = idx;
        }
        j += 1;
    }
    s.add(
        "判序协议：七段严格递增且每段有失败码",
        proto_ok,
        "协议表是判序的单一权威；轨迹不得跑出这张表",
    );

    let t = sample_table();
    let normal_ok = evaluate(&t, mk(Family::Openat, AT_FDCWD, "dir/inner", 0)).code == 0
        && evaluate(&t, mk(Family::Fstatat, FIRST_FD, "dir/inner", 0)).code == 0
        && evaluate(&t, mk(Family::Unlinkat, FIRST_FD, "dir/inner", 0)).code == 0
        && evaluate(&t, mk(Family::Readlinkat, FIRST_FD, "dangling", 0)).code == 0;
    s.add(
        "正常流：四族在合法 fd + 命中路径上全绿",
        normal_ok,
        "先跑通正判据，后面所有坏格才有对照基线",
    );

    let flags_first = evaluate(
        &t,
        Probe { fam: Family::Openat, dirfd: FdClass::Closed.fd(), path: "dir/miss", flags: 0x0008_0000, bad_ptr: true },
    );
    s.add(
        "次序①：坏标志 EINVAL 早于坏指针与坏 fd",
        flags_first.code == EINVAL && flags_first.stage == Stage::Flags,
        "标志段是首段；它失败时轨迹长度只能是 1",
    );

    let ptr_first = evaluate(
        &t,
        Probe { fam: Family::Fstatat, dirfd: FdClass::Closed.fd(), path: "", flags: 0, bad_ptr: true },
    );
    s.add(
        "次序②：坏指针 EFAULT 早于名字段与 fd 段",
        ptr_first.code == EFAULT
            && ptr_first.stage == Stage::Pointer
            && ptr_first.trace.len() == 2
            && !ptr_first.trace.has(Stage::Name),
        "用户指针拷贝失败必须先于路径解析与 fd 校验",
    );

    let empty_ok = evaluate(&t, mk(Family::Openat, FdClass::Closed.fd(), "", 0));
    let empty_flagged = evaluate(&t, mk(Family::Fstatat, FdClass::Closed.fd(), "", AT_EMPTY_PATH));
    s.add(
        "次序③：空路径无标志先 ENOENT，有标志才轮到坏 fd 的 EBADF",
        empty_ok.code == ENOENT
            && empty_ok.stage == Stage::Name
            && !empty_ok.trace.has(Stage::Fd)
            && empty_flagged.code == EBADF
            && empty_flagged.stage == Stage::Fd,
        "getname_flags 在名字段截断；AT_EMPTY_PATH 才把 fd 抬成对象来源",
    );

    let g = run_grid();
    let mut rows_ok = g.mismatches == 0 && g.cells == 24;
    let mut k = 0usize;
    while k < PRECEDENCE.len() {
        if let Some(row) = PRECEDENCE.get(k) {
            let v = evaluate(&t, mk(row.fam, row.fd.fd(), row.path, row.flags));
            if v.code != row.code.val() || v.stage != row.stage {
                rows_ok = false;
            }
        }
        k += 1;
    }
    s.add(
        "对账：24 行 Linux 判序逐例一致（码与段双比对）",
        rows_ok,
        "只比错误码不够；终止段也必须是协议里的那一段",
    );

    let l = run_injection();
    let priority_ok = l.priority_miss == 0
        && l.ebadf == 36
        && evaluate(&t, mk(Family::Openat, FdClass::Closed.fd(), "dir/miss", 0)).code == EBADF
        && evaluate(&t, mk(Family::Fstatat, FdClass::Beyond.fd(), "dir/inner", 0)).code == EBADF
        && evaluate(&t, mk(Family::Unlinkat, FdClass::Negative.fd(), "file/miss", 0)).code == EBADF;
    s.add(
        "坏 fd 优先：36 发相对非空路径全部 EBADF，无一被 ENOENT 抢答",
        priority_ok,
        "三类坏 fd × 四族 × 三条相对路径，逐格都要 EBADF",
    );

    let abs_ok = l.abs_leak == 0
        && evaluate(&t, mk(Family::Openat, FdClass::Negative.fd(), "/dir/miss", 0)).code == ENOENT
        && evaluate(&t, mk(Family::Fstatat, FdClass::Beyond.fd(), "/dir/inner", 0)).code == 0
        && !evaluate(&t, mk(Family::Openat, FdClass::Closed.fd(), "/dir/miss", 0))
            .trace
            .has(Stage::Fd);
    s.add(
        "绝对路径反例：dirfd 整段跳过，坏 fd 不得泄漏成 EBADF",
        abs_ok,
        "path_init 钉 root 后 dirfd 无意义；这条反例是本卡最容易错的一格",
    );

    let nondir_ok = evaluate(&t, mk(Family::Openat, FdClass::File.fd(), "dir/miss", 0)).code
        == ENOTDIR
        && !evaluate(&t, mk(Family::Openat, FdClass::File.fd(), "dir/miss", 0))
            .trace
            .has(Stage::Lookup)
        && evaluate(&t, mk(Family::Fstatat, FdClass::File.fd(), "ghost", 0)).code == ENOTDIR;
    s.add(
        "非目录 fd：ENOTDIR 在目录性段截断，查找段不得被访问",
        nondir_ok,
        "目标缺失也不许降级成 ENOENT；fd 面始终先于路径面",
    );

    let order_ok = l.backtrack == 0 && l.lookup_after_fd == 0;
    s.add(
        "轨迹单调：120 发无回退、EBADF 后无查找段",
        order_ok,
        "trace 是判序的自证材料；出现回退即实现把段重排了",
    );

    let partition_ok = l.conserved()
        && l.shots == 120
        && l.ebadf == 36
        && l.enoent == 56
        && l.enotdir == 20
        && l.ok == 6
        && l.einval == 2;
    s.add(
        "注入账守恒：120 = 36 EBADF + 56 ENOENT + 20 ENOTDIR + 6 OK + 2 EINVAL",
        partition_ok,
        "分桶总和必须等于发数；四类违例全零才允许收账",
    );

    let recon_ok = {
        let mut good = RECON.len() == 16
            && class_of("") == PathClass::Empty
            && class_of("dir/inner") == PathClass::RelPresent
            && class_of("dir/miss") == PathClass::RelMissing
            && class_of("/dir/miss") == PathClass::AbsMissing
            && class_of("file/miss") == PathClass::NonDirPrefix;
        let mut r = 0usize;
        while r < RECON.len() {
            if let Some(row) = RECON.get(r) {
                let v = evaluate(&t, mk(row.fam, row.fd.fd(), row.path, row.flags));
                if v.code != row.code.val() || v.stage != row.stage {
                    good = false;
                }
            }
            r += 1;
        }
        good
    };
    s.add(
        "Linux 逐例复验：16 行含空路径/悬挂链接/标志段反例",
        recon_ok,
        "对账表按 Linux 手册逐条手写，禁止按实现反推",
    );

    let mut cap = FdTable::new();
    let mut opened = true;
    for _ in 0..MAX_FDS {
        if cap.open(ObjKind::Dir).is_err() {
            opened = false;
        }
    }
    let cap_ok = opened
        && cap.open(ObjKind::Dir).is_err()
        && cap.open_count() == MAX_FDS
        && cap.describe(FIRST_FD).is_some()
        && cap.describe(FIRST_FD - 1).is_none();
    s.add(
        "容量：6 槽填满后 EMFILE，fd 号段 3..=8 严格封闭",
        cap_ok,
        "槽位上限是坏 fd 面的边界条件，越界必须走 EBADF 而非猜槽",
    );

    let mut gen = FdTable::new();
    let opened_gen = gen.open(ObjKind::Dir);
    let mut gen_ok = false;
    if let Ok((fd, g1)) = opened_gen {
        let closed_gen = gen.close(fd);
        let reopened = gen.open(ObjKind::File);
        if let (Ok(g2), Ok((fd2, g3))) = (closed_gen, reopened) {
            gen_ok = fd2 == fd
                && g2 == g1 + 1
                && g3 == g2
                && gen.describe_gen(fd, g1) == Err(EBADF)
                && gen.describe_gen(fd, g3) == Ok(ObjKind::File);
        }
    }
    s.add(
        "代际：关闭 +1、复用保留新代际、旧代际 EBADF",
        gen_ok,
        "复用窗口里旧句柄必须失效，代际是 TOCTOU 面的最小防线",
    );

    let dbl_ok = corpus_fingerprint() == corpus_fingerprint() && l.conserved();
    s.add(
        "双跑一致：语料指纹两次相等且账本守恒",
        dbl_ok,
        "确定性是自检可复现的前提；指纹漂移说明有隐藏状态",
    );

    let mut text_ok = true;
    let mut m = 0usize;
    while m < Stage::ALL.len() {
        if let Some(st) = Stage::ALL.get(m) {
            if st.label().is_empty() {
                text_ok = false;
            }
        }
        m += 1;
    }
    text_ok = text_ok && Stage::Flags.label() != Stage::Lookup.label();
    s.add(
        "文案闭集：七段标签非空且互不混同",
        text_ok,
        "诊断文案是给定位的人看的；段名混淆等于没有诊断",
    );

    let grid_text = format!(
        "24格 错{}/EBADF{}/ENOENT{}/ENOTDIR{}/EISDIR{}/EINVAL{}/OK{}",
        g.mismatches, g.ebadf, g.enoent, g.enotdir, g.eisdir, g.einval, g.ok
    );
    s.add_dyn("网格快照", g.mismatches == 0 && g.cells == 24, &grid_text);

    let ledger_text = format!(
        "120发 EBADF{}/ENOENT{}/ENOTDIR{}/OK{}/EINVAL{}/违例{}",
        l.ebadf, l.enoent, l.enotdir, l.ok, l.einval,
        l.lookup_after_fd + l.backtrack + l.abs_leak + l.priority_miss
    );
    s.add_dyn("注入账快照", l.conserved(), &ledger_text);

    let fp1 = corpus_fingerprint();
    let fp_text = format!("语料指纹 0x{:08X}", fp1);
    s.add_dyn("指纹快照", fp1 == corpus_fingerprint(), &fp_text);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_base_guard() {
        assert_eq!(SEG_BASE, 0x7A740);
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x7A000..=0x7D1C0).contains(&SEG_BASE));
    }

    #[test]
    fn code_closed_set() {
        assert_eq!(OrderCode::ALL.len(), 8);
        for c in OrderCode::ALL.iter().copied() {
            assert!(!c.detail().is_empty());
            assert!(!c.label().is_empty());
            assert_eq!(code_of(c.val()), c);
        }
        assert_eq!(OrderCode::Ok.val(), 0);
        assert_eq!(OrderCode::Ebadf.val(), EBADF);
        assert_eq!(OrderCode::Ebadf.label(), "EBADF");
        assert_eq!(OrderCode::Enoent.label(), "ENOENT");
    }

    #[test]
    fn protocol_strictly_increasing() {
        let mut prev = -1i32;
        for rule in PROTOCOL.iter() {
            assert!((rule.stage.idx() as i32) > prev);
            assert_ne!(rule.on_fail, OrderCode::Ok);
            prev = rule.stage.idx() as i32;
        }
    }

    #[test]
    fn normal_flow_all_families() {
        let t = sample_table();
        assert_eq!(evaluate(&t, mk(Family::Openat, FIRST_FD, "dir/inner", 0)).code, 0);
        assert_eq!(evaluate(&t, mk(Family::Fstatat, FIRST_FD, "dir/inner", 0)).code, 0);
        assert_eq!(evaluate(&t, mk(Family::Unlinkat, FIRST_FD, "dir/inner", 0)).code, 0);
        assert_eq!(evaluate(&t, mk(Family::Readlinkat, FIRST_FD, "link", 0)).code, 0);
        assert_eq!(Family::Openat.label(), "openat");
        assert_eq!(Family::Fstatat.label(), "fstatat");
    }

    #[test]
    fn flags_then_pointer_then_name() {
        let t = sample_table();
        let bad_flags = Probe {
            fam: Family::Openat,
            dirfd: FdClass::Closed.fd(),
            path: "dir/miss",
            flags: 0x0008_0000,
            bad_ptr: true,
        };
        let v = evaluate(&t, bad_flags);
        assert_eq!(v.code, EINVAL);
        assert_eq!(v.stage, Stage::Flags);
        assert_eq!(v.trace.len(), 1);

        let bad_ptr = Probe {
            fam: Family::Openat,
            dirfd: FdClass::Closed.fd(),
            path: "",
            flags: 0,
            bad_ptr: true,
        };
        let v2 = evaluate(&t, bad_ptr);
        assert_eq!(v2.code, EFAULT);
        assert_eq!(v2.stage, Stage::Pointer);
        assert!(!v2.trace.has(Stage::Fd));
    }

    #[test]
    fn empty_path_two_faces() {
        let t = sample_table();
        let bare = evaluate(&t, mk(Family::Readlinkat, FdClass::Closed.fd(), "", 0));
        assert_eq!(bare.code, ENOENT);
        assert_eq!(bare.stage, Stage::Name);
        assert!(!bare.trace.has(Stage::Fd));

        let flagged = evaluate(
            &t,
            mk(Family::Readlinkat, FdClass::Closed.fd(), "", AT_EMPTY_PATH),
        );
        assert_eq!(flagged.code, EBADF);
        assert_eq!(flagged.stage, Stage::Fd);

        let valid = evaluate(&t, mk(Family::Readlinkat, FIRST_FD, "", AT_EMPTY_PATH));
        assert_eq!(valid.code, 0);
        assert!(!valid.trace.has(Stage::Lookup));
    }

    #[test]
    fn bad_fd_priority_grid() {
        let t = sample_table();
        for cls in [FdClass::Closed, FdClass::Beyond, FdClass::Negative].iter().copied() {
            for path in ["dir/inner", "dir/miss", "file/miss"].iter().copied() {
                for fam in FAMILIES.iter().copied() {
                    let v = evaluate(&t, mk(fam, cls.fd(), path, 0));
                    assert_eq!(v.code, EBADF, "{:?} {:?} {}", fam, cls, path);
                    assert_eq!(v.stage, Stage::Fd);
                    assert!(!v.trace.has(Stage::Lookup));
                }
            }
        }
    }

    #[test]
    fn absolute_path_ignores_dirfd() {
        let t = sample_table();
        let v = evaluate(&t, mk(Family::Openat, FdClass::Negative.fd(), "/dir/miss", 0));
        assert_eq!(v.code, ENOENT);
        assert_eq!(v.stage, Stage::Lookup);
        assert!(!v.trace.has(Stage::Fd));
        assert!(!v.trace.has(Stage::DirKind));

        let present = evaluate(&t, mk(Family::Fstatat, FdClass::Beyond.fd(), "/dir/inner", 0));
        assert_eq!(present.code, 0);
    }

    #[test]
    fn non_dir_fd_before_lookup() {
        let t = sample_table();
        let v = evaluate(&t, mk(Family::Openat, FdClass::File.fd(), "dir/miss", 0));
        assert_eq!(v.code, ENOTDIR);
        assert_eq!(v.stage, Stage::DirKind);
        assert!(!v.trace.has(Stage::Lookup));
    }

    #[test]
    fn object_stage_forms() {
        let t = sample_table();
        assert_eq!(
            evaluate(&t, mk(Family::Openat, FIRST_FD, "dir", 0x1)).code,
            EISDIR
        );
        assert_eq!(
            evaluate(&t, mk(Family::Unlinkat, FIRST_FD, "dir", 0)).code,
            EISDIR
        );
        assert_eq!(
            evaluate(&t, mk(Family::Unlinkat, FIRST_FD, "dir", AT_REMOVEDIR)).code,
            0
        );
        assert_eq!(
            evaluate(&t, mk(Family::Readlinkat, FIRST_FD, "dir/inner", 0)).code,
            EINVAL
        );
    }

    #[test]
    fn precedence_rows() {
        let t = sample_table();
        for row in PRECEDENCE.iter() {
            let v = evaluate(&t, mk(row.fam, row.fd.fd(), row.path, row.flags));
            assert_eq!(v.code, row.code.val(), "{:?}", row.path);
            assert_eq!(v.stage, row.stage, "{:?}", row.path);
        }
    }

    #[test]
    fn recon_rows() {
        let t = sample_table();
        for row in RECON.iter() {
            let v = evaluate(&t, mk(row.fam, row.fd.fd(), row.path, row.flags));
            assert_eq!(v.code, row.code.val(), "{:?} {:?}", row.fam, row.path);
            assert_eq!(v.stage, row.stage, "{:?} {:?}", row.fam, row.path);
        }
    }

    #[test]
    fn injection_partition() {
        let l = run_injection();
        assert_eq!(l.shots, 120);
        assert_eq!(l.ebadf, 36);
        assert_eq!(l.enoent, 56);
        assert_eq!(l.enotdir, 20);
        assert_eq!(l.ok, 6);
        assert_eq!(l.einval, 2);
        assert!(l.conserved());
        assert_eq!(l.priority_miss, 0);
        assert_eq!(l.abs_leak, 0);
        assert_eq!(l.backtrack, 0);
        assert_eq!(l.lookup_after_fd, 0);
        for cls in FdClass::ALL.iter().copied() {
            assert!(!cls.label().is_empty());
        }
        assert_eq!(FdClass::Cwd.label(), "AT_FDCWD");
    }

    #[test]
    fn capacity_and_generation() {
        let mut t = FdTable::new();
        for _ in 0..MAX_FDS {
            assert!(t.open(ObjKind::Dir).is_ok());
        }
        assert_eq!(t.open(ObjKind::Dir), Err(EMFILE));
        assert_eq!(t.open_count(), MAX_FDS);

        let mut g = FdTable::new();
        let (fd, first) = g.open(ObjKind::Dir).unwrap_or((-1, 0));
        let second = g.close(fd).unwrap_or(0);
        assert_eq!(second, first + 1);
        let (fd2, third) = g.open(ObjKind::File).unwrap_or((-1, 0));
        assert_eq!(fd2, fd);
        assert_eq!(third, second);
        assert_eq!(g.describe_gen(fd, first), Err(EBADF));
        assert_eq!(g.describe_gen(fd, third), Ok(ObjKind::File));
        assert_eq!(g.close(FdClass::Beyond.fd()), Err(EBADF));
        assert_eq!(g.close(FdClass::Negative.fd()), Err(EBADF));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
        assert_eq!(run_grid().cells, 24);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9231_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(s.len() >= 12);
        assert!(!s.truncated());
    }
}
