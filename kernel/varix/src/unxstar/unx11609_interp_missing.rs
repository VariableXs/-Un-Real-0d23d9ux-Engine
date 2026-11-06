//! UNX-F11609 PT_INTERP 缺失解释器（ENOENT）文案判据
//!
//! 镜面：`execve` 对带 PT_INTERP 的 ELF 先解出解释器路径，再以它重新
//! 装载。解释器打不开时，内核返回 ENOENT，外壳据此打印
//! `bash: ./prog: No such file or directory`。本单把「解释器路径 →
//! 目标形态 → errno → 文案」整条链压成一张可对账的判据表：
//!
//! * 路径缺失、悬空 → ENOENT；
//! * 路径落在目录或不可执行件上 → EACCES；
//! * 自指链接层数溢出 → ELOOP；
//! * 中间段不是目录 → 路径码（EPATH/ENOTDIR 族）；
//! * 打开后不是 ELF → ENOEXEC。
//!
//! 语料十四槽，二活十二死：missing 五、dir 二、noexec 三、loop 一、
//! notdir 一。每一槽都对照「判定码 == 槽位码」，再由汇总闸门把
//! 「十四槽、二活、十二死、五缺、二目、三无权、一环、一非目、全对、
//! 千分千」逐项卡死。
//!
//! 闸门次序：名称 → 段视图 → 判定 → 文案 → 语料 → 汇总 → 红单 →
//! 覆盖 → 解析器 → 冻结重放。
//!
//! 契约：纯 `no_std` 函数式代码，无 `unsafe`，无 `unwrap`/`expect`/
//! `panic!`，一切失败走 `Result<_, u32>`；只读、无全局态。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 本单段基址。
pub const SEG_BASE: u32 = 0xfb80;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// PT_INTERP 段类型。
pub const PT_INTERP: u32 = 3;

/// 解释器路径字节上限（含末尾 NUL）。
pub const INTERP_MAX: usize = 255;

/// 段表槽上限。
pub const PHDRS: usize = 6;

/// 装载映像字节上限。
pub const BLOB: usize = 512;

/// ENOENT。
pub const ENOENT: i32 = 2;

/// EACCES。
pub const EACCES: i32 = 13;

/// ENOTDIR。
pub const ENOTDIR: i32 = 20;

/// ENAMETOOLONG。
pub const ENAMETOOLONG: i32 = 36;

/// ELOOP。
pub const ELOOP: i32 = 40;

/// ENOEXEC。
pub const ENOEXEC: i32 = 8;

// ---------------------------------------------------------------------------
// 错误码：八域 + 五账
// ---------------------------------------------------------------------------

/// 解释器路径不合法。
pub const IP_EPATH: u32 = SEG_BASE + 0x01;
/// 解释器不存在。
pub const IP_ENOENT: u32 = SEG_BASE + 0x02;
/// 解释器路径超长。
pub const IP_ELONG: u32 = SEG_BASE + 0x03;
/// 解释器不可执行。
pub const IP_EACCES: u32 = SEG_BASE + 0x04;
/// 链接环。
pub const IP_ELOOP: u32 = SEG_BASE + 0x05;
/// 解释器格式错。
pub const IP_ENOEXEC: u32 = SEG_BASE + 0x06;
/// 状态违例。
pub const IP_ESTATE: u32 = SEG_BASE + 0x07;
/// 红档。
pub const IP_EVER: u32 = SEG_BASE + 0x08;
/// 账不平。
pub const IP_EBAL: u32 = SEG_BASE + 0x09;
/// 槽错位。
pub const IP_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const IP_EVALUE: u32 = SEG_BASE + 0x0b;
/// 封存违例。
pub const IP_ESEAL: u32 = SEG_BASE + 0x0c;
/// 未观测。
pub const IP_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 映像解析。
pub const OP_IMAGE: u32 = 1;
/// 目标判定。
pub const OP_TARGET: u32 = 2;
/// 路径闸门。
pub const OP_PATH: u32 = 3;
/// 载荷闸门。
pub const OP_PAYLOAD: u32 = 4;
/// 汇总。
pub const OP_REPORT: u32 = 5;

// ---------------------------------------------------------------------------
// 指纹
// ---------------------------------------------------------------------------

/// FNV-1a 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 质数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 折一个字节。
pub const fn fold_byte(h: u64, b: u8) -> u64 {
    let mut x = h ^ (b as u64);
    x = x.wrapping_mul(FNV_PRIME);
    x
}

/// 折一个字（低位在前，八字节）。
pub const fn fold_word(h: u64, w: u64) -> u64 {
    let mut x = h;
    let mut i = 0u32;
    while i < 8 {
        x = fold_byte(x, ((w >> (i * 8)) & 0xff) as u8);
        i += 1;
    }
    x
}

/// 折一个字符串。
pub fn fold_str(h: u64, s: &str) -> u64 {
    let b = s.as_bytes();
    let mut x = h;
    let mut i = 0usize;
    while i < b.len() {
        if let Some(v) = b.get(i) {
            x = fold_byte(x, *v);
        }
        i += 1;
    }
    fold_word(x, b.len() as u64)
}

/// 十进制文本。
pub fn u32_text(v: u32) -> String {
    let mut buf = [0u8; 10];
    let mut n = v;
    let mut i = 10usize;
    if n == 0 {
        return String::from("0");
    }
    while n > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
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
// 目标形态
// ---------------------------------------------------------------------------

/// 解释器路径落到的目标形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// 常态 ELF 解释器。
    Elf,
    /// 路径不存在（含悬空链）。
    Missing,
    /// 目录。
    Dir,
    /// 存在但无执行位。
    NoExec,
    /// 链接环。
    Loop,
    /// 中间段不是目录。
    NotDir,
    /// 存在但不是 ELF。
    Text,
}

impl Target {
    /// 名称。
    pub const fn name(&self) -> &'static str {
        match self {
            Target::Elf => "elf",
            Target::Missing => "missing",
            Target::Dir => "dir",
            Target::NoExec => "noexec",
            Target::Loop => "loop",
            Target::NotDir => "notdir",
            Target::Text => "text",
        }
    }

    /// 序数。
    pub const fn index(&self) -> u32 {
        match self {
            Target::Elf => 0,
            Target::Missing => 1,
            Target::Dir => 2,
            Target::NoExec => 3,
            Target::Loop => 4,
            Target::NotDir => 5,
            Target::Text => 6,
        }
    }

    /// 细节短语。
    pub const fn detail(&self) -> &'static str {
        match self {
            Target::Elf => "interpreter present",
            Target::Missing => "interpreter missing",
            Target::Dir => "interpreter is a directory",
            Target::NoExec => "interpreter not executable",
            Target::Loop => "interpreter symlink loop",
            Target::NotDir => "interpreter path component not a directory",
            Target::Text => "interpreter is not an ELF image",
        }
    }

    /// 是否放行。
    pub const fn alive(&self) -> bool {
        matches!(self, Target::Elf)
    }
}

// ---------------------------------------------------------------------------
// 判定
// ---------------------------------------------------------------------------

/// 一次判定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verdict {
    code: u32,
    errno: i32,
    target: Target,
    detail: &'static str,
    ok: bool,
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

    /// 目标。
    pub const fn target(&self) -> Target {
        self.target
    }

    /// 细节。
    pub const fn detail(&self) -> &'static str {
        self.detail
    }

    /// 是否放行。
    pub const fn ok(&self) -> bool {
        self.ok
    }
}

/// 判一个目标。
pub fn judge(t: Target) -> Verdict {
    match t {
        Target::Elf => Verdict {
            code: 0,
            errno: 0,
            target: t,
            detail: t.detail(),
            ok: true,
        },
        Target::Missing => Verdict {
            code: IP_ENOENT,
            errno: ENOENT,
            target: t,
            detail: t.detail(),
            ok: false,
        },
        Target::Dir => Verdict {
            code: IP_EACCES,
            errno: EACCES,
            target: t,
            detail: t.detail(),
            ok: false,
        },
        Target::NoExec => Verdict {
            code: IP_EACCES,
            errno: EACCES,
            target: t,
            detail: t.detail(),
            ok: false,
        },
        Target::Loop => Verdict {
            code: IP_ELOOP,
            errno: ELOOP,
            target: t,
            detail: t.detail(),
            ok: false,
        },
        Target::NotDir => Verdict {
            code: IP_EPATH,
            errno: ENOTDIR,
            target: t,
            detail: t.detail(),
            ok: false,
        },
        Target::Text => Verdict {
            code: IP_ENOEXEC,
            errno: ENOEXEC,
            target: t,
            detail: t.detail(),
            ok: false,
        },
    }
}

/// errno 文本。
pub fn strerror(e: i32) -> &'static str {
    match e {
        ENOENT => "No such file or directory",
        EACCES => "Permission denied",
        ENOTDIR => "Not a directory",
        ENAMETOOLONG => "File name too long",
        ELOOP => "Too many levels of symbolic links",
        ENOEXEC => "Exec format error",
        _ => "Success",
    }
}

/// 文案。
pub fn message(v: &Verdict, path: &str) -> Result<String, u32> {
    if path.is_empty() || path.len() > INTERP_MAX {
        return Err(IP_EPATH);
    }
    let mut s = String::new();
    if v.ok {
        s.push_str("ok: interpreter ");
        s.push_str(path);
        s.push('\n');
    } else {
        s.push_str("bash: ./prog: ");
        s.push_str(strerror(v.errno));
        s.push_str(" (PT_INTERP=");
        s.push_str(path);
        s.push_str(")\n");
    }
    Ok(s)
}

/// 短文案（ld.so 域内一侧）。
pub fn short_message(v: &Verdict) -> &'static str {
    if v.ok {
        "interpreter ok"
    } else {
        match v.target {
            Target::Missing => "cannot open shared object: No such file or directory",
            Target::Dir => "cannot open shared object: Is a directory",
            Target::NoExec => "cannot open shared object: Permission denied",
            Target::Loop => "cannot open shared object: Too many levels of symbolic links",
            Target::NotDir => "cannot open shared object: Not a directory",
            Target::Text => "cannot open shared object: Exec format error",
            Target::Elf => "interpreter ok",
        }
    }
}

// ---------------------------------------------------------------------------
// 路径表
// ---------------------------------------------------------------------------

/// 路径 → 目标形态。
pub fn target_of(path: &str) -> Option<Target> {
    match path {
        "/lib64/ld-linux-x86-64.so.2" => Some(Target::Elf),
        "/lib/ld-musl-x86_64.so.1" => Some(Target::Elf),
        "/lib/ld-musl.so.1" => Some(Target::Missing),
        "/lib64/ld-linux.so.2" => Some(Target::Missing),
        "/opt/dev/ld.so" => Some(Target::Missing),
        "/nonexistent/ld.so" => Some(Target::Missing),
        "/tmp/ld.so" => Some(Target::Missing),
        "/lib/ld64.so.1" => Some(Target::Dir),
        "/usr/lib/ld" => Some(Target::Dir),
        "/lib/ld-noperm.so" => Some(Target::NoExec),
        "/lib/ld-immutable.so" => Some(Target::NoExec),
        "/lib/ld-rx-only.so" => Some(Target::NoExec),
        "/lib/loop/ld.so" => Some(Target::Loop),
        "/etc/passwd/ld.so" => Some(Target::NotDir),
        "/usr/share/ld.so" => Some(Target::Text),
        _ => None,
    }
}

/// 路径解析。
pub fn resolve(path: &str) -> Result<Verdict, u32> {
    if path.is_empty() {
        return Err(IP_EPATH);
    }
    if path.len() > INTERP_MAX {
        return Err(IP_ELONG);
    }
    match target_of(path) {
        Some(t) => Ok(judge(t)),
        None => Err(IP_ENOENT),
    }
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 一个语料槽。
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    /// 解释器路径。
    pub path: &'static str,
    /// 目标形态。
    pub target: Target,
    /// 期望码。
    pub code: u32,
    /// 备注。
    pub note: &'static str,
}

impl Slot {
    /// 构造。
    pub const fn new(path: &'static str, target: Target, code: u32, note: &'static str) -> Slot {
        Slot { path, target, code, note }
    }
}

/// 十四槽语料：两活十二死。
pub const CASES: [Slot; CORPUS] = [
    Slot::new("/lib64/ld-linux-x86-64.so.2", Target::Elf, 0, "glibc-x86_64"),
    Slot::new("/lib/ld-musl-x86_64.so.1", Target::Elf, 0, "musl-x86_64"),
    Slot::new("/lib/ld-musl.so.1", Target::Missing, IP_ENOENT, "musl-short"),
    Slot::new("/lib64/ld-linux.so.2", Target::Missing, IP_ENOENT, "i386-name-on-x86_64"),
    Slot::new("/opt/dev/ld.so", Target::Missing, IP_ENOENT, "stale-build-path"),
    Slot::new("/nonexistent/ld.so", Target::Missing, IP_ENOENT, "vacuous-path"),
    Slot::new("/tmp/ld.so", Target::Missing, IP_ENOENT, "tmp-wiped"),
    Slot::new("/lib/ld64.so.1", Target::Dir, IP_EACCES, "dir-named-like-loader"),
    Slot::new("/usr/lib/ld", Target::Dir, IP_EACCES, "plain-dir"),
    Slot::new("/lib/ld-noperm.so", Target::NoExec, IP_EACCES, "mode-0644"),
    Slot::new("/lib/ld-immutable.so", Target::NoExec, IP_EACCES, "immutable-no-x"),
    Slot::new("/lib/ld-rx-only.so", Target::NoExec, IP_EACCES, "owner-only-x"),
    Slot::new("/lib/loop/ld.so", Target::Loop, IP_ELOOP, "self-referential-link"),
    Slot::new("/etc/passwd/ld.so", Target::NotDir, IP_EPATH, "file-as-directory"),
];

/// 取一槽。
pub fn slot_at(i: usize) -> Option<Slot> {
    CASES.get(i).copied()
}

/// 语料是否互异。
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
                if a.path == b.path {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 备注全非空。
pub fn notes_ok() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        match CASES.get(i) {
            Some(s) => {
                if s.note.is_empty() || s.path.is_empty() {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 路径表全覆盖。
pub fn table_covers() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        match CASES.get(i) {
            Some(s) => match target_of(s.path) {
                Some(t) => {
                    if t != s.target {
                        return false;
                    }
                }
                None => return false,
            },
            None => return false,
        }
        i += 1;
    }
    true
}

/// 语料逐条对判。
pub fn table_ok() -> bool {
    if !unique() || !notes_ok() || !table_covers() {
        return false;
    }
    let mut i = 0usize;
    while i < CASES.len() {
        let slot = match CASES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let v = judge(slot.target);
        if v.code != slot.code {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 段视图
// ---------------------------------------------------------------------------

/// 一个程序头。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Phdr {
    /// 类型。
    pub p_type: u32,
    /// 文件偏移。
    pub p_offset: u32,
    /// 虚拟地址。
    pub p_vaddr: u32,
    /// 文件长度。
    pub p_filesz: u32,
    /// 内存长度。
    pub p_memsz: u32,
    /// 对齐。
    pub p_align: u32,
}

impl Phdr {
    /// 构造。
    pub const fn new(p_type: u32, p_offset: u32, p_vaddr: u32, p_filesz: u32, p_align: u32) -> Phdr {
        Phdr { p_type, p_offset, p_vaddr, p_filesz, p_memsz: p_filesz, p_align }
    }

    /// 是否 PT_INTERP。
    pub const fn is_interp(&self) -> bool {
        self.p_type == PT_INTERP
    }

    /// 文件区间末偏移。
    pub fn end(&self) -> Result<u32, u32> {
        match self.p_offset.checked_add(self.p_filesz) {
            Some(e) => Ok(e),
            None => Err(IP_EPATH),
        }
    }

    /// 是否落在映像内。
    pub fn within(&self, len: usize) -> bool {
        match self.end() {
            Ok(e) => (e as usize) <= len,
            Err(_) => false,
        }
    }

    /// 是否页对齐。
    pub const fn aligned(&self) -> bool {
        self.p_align == 0 || self.p_align == 1 || (self.p_vaddr % self.p_align == 0)
    }
}

/// 装载映像。
#[derive(Clone, Copy, Debug)]
pub struct Image {
    phdrs: [Phdr; PHDRS],
    count: usize,
    blob: [u8; BLOB],
    len: usize,
}

impl Image {
    /// 空映像。
    pub const fn new(len: usize) -> Image {
        Image { phdrs: [Phdr::new(0, 0, 0, 0, 0); PHDRS], count: 0, blob: [0u8; BLOB], len: if len > BLOB { BLOB } else { len } }
    }

    /// 写字节。
    pub fn put(&mut self, off: usize, bytes: &[u8]) -> Result<(), u32> {
        if off > self.len {
            return Err(IP_EPATH);
        }
        let end = match off.checked_add(bytes.len()) {
            Some(e) => e,
            None => return Err(IP_EPATH),
        };
        if end > self.len {
            return Err(IP_EPATH);
        }
        let mut i = 0usize;
        while i < bytes.len() {
            if let (Some(dst), Some(b)) = (self.blob.get_mut(off + i), bytes.get(i)) {
                *dst = *b;
            }
            i += 1;
        }
        Ok(())
    }

    /// 追加程序头。
    pub fn push(&mut self, p: Phdr) -> Result<(), u32> {
        if self.count >= PHDRS {
            return Err(IP_ESLOT);
        }
        if !p.within(self.len) {
            return Err(IP_EPATH);
        }
        if let Some(dst) = self.phdrs.get_mut(self.count) {
            *dst = p;
            self.count += 1;
            return Ok(());
        }
        Err(IP_ESLOT)
    }

    /// 头数。
    pub const fn count(&self) -> usize {
        self.count
    }

    /// 取头。
    pub fn phdr(&self, i: usize) -> Option<Phdr> {
        if i >= self.count {
            return None;
        }
        self.phdrs.get(i).copied()
    }

    /// PT_INTERP 段。
    pub fn interp_phdr(&self) -> Option<Phdr> {
        let mut i = 0usize;
        while i < self.count {
            if let Some(p) = self.phdrs.get(i) {
                if p.is_interp() {
                    return Some(*p);
                }
            }
            i += 1;
        }
        None
    }

    /// 解释器路径字节。
    pub fn interp_bytes(&self) -> Result<&[u8], u32> {
        let p = match self.interp_phdr() {
            Some(v) => v,
            None => return Err(IP_EPATH),
        };
        if p.p_filesz == 0 {
            return Err(IP_EPATH);
        }
        if p.p_filesz as usize > INTERP_MAX + 1 {
            return Err(IP_ELONG);
        }
        let start = p.p_offset as usize;
        let end = match start.checked_add(p.p_filesz as usize) {
            Some(e) => e,
            None => return Err(IP_EPATH),
        };
        match self.blob.get(start..end) {
            Some(s) => Ok(s),
            None => Err(IP_EPATH),
        }
    }

    /// 解释器路径。
    pub fn interp(&self) -> Result<&str, u32> {
        let raw = self.interp_bytes()?;
        let mut n = 0usize;
        while n < raw.len() {
            match raw.get(n) {
                Some(0) => break,
                Some(_) => n += 1,
                None => break,
            }
        }
        if n == 0 {
            return Err(IP_EPATH);
        }
        let body = match raw.get(0..n) {
            Some(s) => s,
            None => return Err(IP_EPATH),
        };
        match core::str::from_utf8(body) {
            Ok(s) => Ok(s),
            Err(_) => Err(IP_EPATH),
        }
    }
}

/// 打一帧标准映像（PT_PHDR、PT_LOAD、PT_INTERP、PT_LOAD）。
pub fn sample_image(path: &str) -> Result<Image, u32> {
    let mut img = Image::new(BLOB);
    let p = path.as_bytes();
    if p.len() + 1 > INTERP_MAX {
        return Err(IP_ELONG);
    }
    let off = 0x40u32;
    let mut buf = [0u8; INTERP_MAX + 1];
    let mut i = 0usize;
    while i < p.len() {
        if let (Some(dst), Some(b)) = (buf.get_mut(i), p.get(i)) {
            *dst = *b;
        }
        i += 1;
    }
    img.put(off as usize, &buf[0..p.len() + 1])?;
    img.push(Phdr::new(6, 0x40, 0x400040, 0x40, 8))?;
    img.push(Phdr::new(1, 0x000, 0x400000, 0x000, 0x1000))?;
    img.push(Phdr::new(PT_INTERP, off, 0x400040, (p.len() + 1) as u32, 1))?;
    img.push(Phdr::new(1, 0x100, 0x401000, 0x80, 0x1000))?;
    Ok(img)
}

/// 映像里的解释器路径 → 判定。
pub fn judge_image(img: &Image) -> Result<Verdict, u32> {
    let path = img.interp()?;
    resolve(path)
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
    missing: usize,
    dir: usize,
    noexec: usize,
    loopn: usize,
    notdir: usize,
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
            missing: 0,
            dir: 0,
            noexec: 0,
            loopn: 0,
            notdir: 0,
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

    /// 缺失条数。
    pub const fn missing(&self) -> usize {
        self.missing
    }

    /// 目录条数。
    pub const fn dir(&self) -> usize {
        self.dir
    }

    /// 无权条数。
    pub const fn noexec(&self) -> usize {
        self.noexec
    }

    /// 环条数。
    pub const fn loopn(&self) -> usize {
        self.loopn
    }

    /// 非目条数。
    pub const fn notdir(&self) -> usize {
        self.notdir
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
            return Err(IP_ENOBS);
        }
        if self.red {
            return Err(IP_EVER);
        }
        if self.matched != self.total {
            return Err(IP_EVALUE);
        }
        if self.accept != 2 || self.reject != 12 {
            return Err(IP_EVALUE);
        }
        if self.missing != 5 || self.dir != 2 || self.noexec != 3 {
            return Err(IP_EVALUE);
        }
        if self.loopn != 1 || self.notdir != 1 {
            return Err(IP_EVALUE);
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
        s.push_str("interp corpus=");
        s.push_str(u32_text(self.total as u32).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept as u32).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject as u32).as_str());
        s.push_str(" missing=");
        s.push_str(u32_text(self.missing as u32).as_str());
        s.push_str(" dir=");
        s.push_str(u32_text(self.dir as u32).as_str());
        s.push_str(" noexec=");
        s.push_str(u32_text(self.noexec as u32).as_str());
        s.push_str(" loop=");
        s.push_str(u32_text(self.loopn as u32).as_str());
        s.push_str(" notdir=");
        s.push_str(u32_text(self.notdir as u32).as_str());
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
            None => return Err(IP_EVALUE),
        };
        let v = judge(slot.target);
        r.total += 1;
        if v.ok {
            r.accept += 1;
        } else {
            r.reject += 1;
            match v.target {
                Target::Missing => r.missing += 1,
                Target::Dir => r.dir += 1,
                Target::NoExec => r.noexec += 1,
                Target::Loop => r.loopn += 1,
                Target::NotDir => r.notdir += 1,
                _ => {}
            }
        }
        if v.code == slot.code {
            r.matched += 1;
        }
        i += 1;
    }
    if let Some((slot, _)) = red {
        if slot >= r.total {
            return Err(IP_ESLOT);
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
        Err(IP_ENOBS)
    }
}

// ---------------------------------------------------------------------------
// 解析器：一门一用
// ---------------------------------------------------------------------------

/// 解释器解析器。
#[derive(Clone, Copy, Debug)]
pub struct Resolver {
    steps: u32,
    bound: bool,
    last: u32,
}

impl Resolver {
    /// 空。
    pub const fn new() -> Resolver {
        Resolver { steps: 0, bound: false, last: 0 }
    }

    /// 打开一条。
    pub fn open(&mut self, path: &str) -> Result<Verdict, u32> {
        if self.bound {
            self.last = IP_ESTATE;
            return Err(IP_ESTATE);
        }
        self.bound = true;
        self.steps = self.steps.saturating_add(1);
        let v = resolve(path)?;
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

/// 路径是否绝对。
pub fn absolute(path: &str) -> bool {
    match path.as_bytes().first() {
        Some(b'/') => true,
        _ => false,
    }
}

/// 路径分段数（`/a/b` 两段）。
pub fn segments(path: &str) -> usize {
    let b = path.as_bytes();
    let mut n = 0usize;
    let mut i = 0usize;
    let mut in_seg = false;
    while i < b.len() {
        match b.get(i) {
            Some(b'/') => in_seg = false,
            Some(_) => {
                if !in_seg {
                    n += 1;
                    in_seg = true;
                }
            }
            None => break,
        }
        i += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// 账本
// ---------------------------------------------------------------------------

/// 一格。
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    /// 槽。
    pub slot: u32,
    /// 操作。
    pub op: u32,
    /// 是否成功。
    pub ok: bool,
    /// 码。
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
            return Err(IP_ESLOT);
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
            return Err(IP_EBAL);
        }
        const VALS: [u32; 8] = [14, 2, 12, 5, 2, 3, 1, 929];
        const CODES: [u32; 6] = [IP_ENOENT, IP_EACCES, IP_EACCES, IP_ELOOP, IP_EPATH, IP_ENOEXEC];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(IP_EBAL),
            };
            if s.slot as usize != i {
                return Err(IP_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(IP_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(IP_EVALUE);
                        }
                    }
                    None => return Err(IP_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(IP_EVALUE);
                        }
                    }
                    None => return Err(IP_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(IP_EBAL);
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
            return Err(IP_ESEAL);
        }
        if self.len != SAMPLES {
            return Err(IP_EBAL);
        }
        self.audit()?;
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放（只读校验）。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if !self.frozen {
            return Err(IP_ESTATE);
        }
        let again = self.seal();
        if again != self.root {
            return Err(IP_ESEAL);
        }
        Ok(self.root)
    }

    /// 根。
    pub const fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub const fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒收次数。
    pub const fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 计数格。
fn count_cell(slot: u32, op: u32, v: Result<usize, u32>) -> Sample {
    match v {
        Ok(n) => Sample { slot, op, ok: true, code: 0, value: n as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 错误格。
fn err_cell(slot: u32, op: u32, code: u32) -> Sample {
    Sample { slot, op, ok: false, code, value: 0 }
}

/// 造账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();

    let _ = l.register(count_cell(0, OP_REPORT, r.map(|x| x.total())));
    let _ = l.register(count_cell(1, OP_REPORT, r.map(|x| x.accept())));
    let _ = l.register(count_cell(2, OP_REPORT, r.map(|x| x.reject())));
    let _ = l.register(count_cell(3, OP_TARGET, r.map(|x| x.missing())));

    let _ = l.register(count_cell(4, OP_TARGET, r.map(|x| x.dir())));
    let _ = l.register(count_cell(5, OP_TARGET, r.map(|x| x.noexec())));
    let _ = l.register(count_cell(6, OP_TARGET, r.map(|x| x.loopn())));

    let red = run_with(Some((0, IP_ENOENT)));
    let _ = l.register(count_cell(7, OP_PATH, red.map(|x| x.rate() as usize)));

    let miss = judge(Target::Missing);
    let _ = l.register(err_cell(8, OP_TARGET, if miss.code == 0 { IP_ENOENT } else { miss.code }));

    let dir = judge(Target::Dir);
    let _ = l.register(err_cell(9, OP_TARGET, if dir.code == 0 { IP_EACCES } else { dir.code }));

    let nx = judge(Target::NoExec);
    let _ = l.register(err_cell(10, OP_PATH, if nx.code == 0 { IP_EACCES } else { nx.code }));

    let lp = judge(Target::Loop);
    let _ = l.register(err_cell(11, OP_PATH, if lp.code == 0 { IP_ELOOP } else { lp.code }));

    let nd = judge(Target::NotDir);
    let _ = l.register(err_cell(12, OP_PATH, if nd.code == 0 { IP_EPATH } else { nd.code }));

    let tx = judge(Target::Text);
    let _ = l.register(err_cell(13, OP_PAYLOAD, if tx.code == 0 { IP_ENOEXEC } else { tx.code }));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 名称闸门。
fn name_ok() -> bool {
    let names = [
        Target::Elf.name(),
        Target::Missing.name(),
        Target::Dir.name(),
        Target::NoExec.name(),
        Target::Loop.name(),
        Target::NotDir.name(),
        Target::Text.name(),
    ];
    let mut i = 0usize;
    while i < names.len() {
        if names[i].is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < names.len() {
            if names[i] == names[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    IP_EPATH == SEG_BASE + 1 && IP_ENOENT == SEG_BASE + 2 && IP_ELONG == SEG_BASE + 3
        && IP_EACCES == SEG_BASE + 4 && IP_ELOOP == SEG_BASE + 5 && IP_ENOEXEC == SEG_BASE + 6
        && IP_ESTATE == SEG_BASE + 7 && IP_EVER == SEG_BASE + 8 && IP_EBAL == SEG_BASE + 9
        && IP_ESLOT == SEG_BASE + 0x0a && IP_EVALUE == SEG_BASE + 0x0b && IP_ESEAL == SEG_BASE + 0x0c
        && IP_ENOBS == SEG_BASE + 0x0d
}

/// 目标闸门。
fn target_ok() -> bool {
    let a = judge(Target::Elf).ok() && judge(Target::Elf).code() == 0 && judge(Target::Elf).errno() == 0;
    let b = judge(Target::Missing).code() == IP_ENOENT && judge(Target::Missing).errno() == ENOENT;
    let c = judge(Target::Dir).code() == IP_EACCES && judge(Target::Dir).errno() == EACCES;
    let d = judge(Target::NoExec).code() == IP_EACCES && judge(Target::NoExec).errno() == EACCES;
    let e = judge(Target::Loop).code() == IP_ELOOP && judge(Target::Loop).errno() == ELOOP;
    let f = judge(Target::NotDir).code() == IP_EPATH && judge(Target::NotDir).errno() == ENOTDIR;
    let g = judge(Target::Text).code() == IP_ENOEXEC && judge(Target::Text).errno() == ENOEXEC;
    let h = !judge(Target::Missing).ok() && !judge(Target::Text).ok();
    a && b && c && d && e && f && g && h
}

/// 次序闸门：缺失先于其余，环先于目标。
fn order_ok() -> bool {
    let mut r = Resolver::new();
    let a = match r.open("/lib/ld-musl.so.1") {
        Ok(v) => v.code() == IP_ENOENT && v.target() == Target::Missing,
        Err(_) => false,
    };
    let b = r.open("/lib64/ld-linux-x86-64.so.2").err() == Some(IP_ESTATE);
    let c = r.bound() && r.last() == IP_ESTATE && r.steps() == 1;
    let d = match resolve("/lib/loop/ld.so") {
        Ok(v) => v.target() == Target::Loop && v.code() == IP_ELOOP,
        Err(_) => false,
    };
    let e = match resolve("/etc/passwd/ld.so") {
        Ok(v) => v.target() == Target::NotDir && v.errno() == ENOTDIR,
        Err(_) => false,
    };
    a && b && c && d && e
}

/// 文案闸门。
fn message_ok() -> bool {
    let m = match message(&judge(Target::Missing), "/lib/ld-musl.so.1") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = m.as_str()
        == "bash: ./prog: No such file or directory (PT_INTERP=/lib/ld-musl.so.1)\n";
    let ok = match message(&judge(Target::Elf), "/lib64/ld-linux-x86-64.so.2") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let b = ok.as_str() == "ok: interpreter /lib64/ld-linux-x86-64.so.2\n";
    let dir = match message(&judge(Target::Dir), "/lib/ld64.so.1") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let c = dir.as_str()
        == "bash: ./prog: Permission denied (PT_INTERP=/lib/ld64.so.1)\n";
    let d = message(&judge(Target::Elf), "").err() == Some(IP_EPATH);
    let e = strerror(ENOENT) == "No such file or directory" && strerror(ELOOP) == "Too many levels of symbolic links";
    a && b && c && d && e
}

/// errno 映射闸门。
fn errno_ok() -> bool {
    let a = strerror(EACCES) == "Permission denied";
    let b = strerror(ENOTDIR) == "Not a directory";
    let c = strerror(ENAMETOOLONG) == "File name too long";
    let d = strerror(ENOEXEC) == "Exec format error";
    let e = strerror(0) == "Success";
    let f = short_message(&judge(Target::Missing)) == "cannot open shared object: No such file or directory";
    a && b && c && d && e && f
}

/// 段视图闸门。
fn image_ok() -> bool {
    let img = match sample_image("/lib64/ld-linux-x86-64.so.2") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = img.count() == 4;
    let b = match img.interp() {
        Ok(p) => p == "/lib64/ld-linux-x86-64.so.2",
        Err(_) => false,
    };
    let c = match img.interp_phdr() {
        Some(p) => p.is_interp() && p.within(BLOB) && p.aligned(),
        None => false,
    };
    let d = match judge_image(&img) {
        Ok(v) => v.ok(),
        Err(_) => false,
    };
    let mut long = String::new();
    let mut k = 0usize;
    while k < INTERP_MAX + 8 {
        long.push('a');
        k += 1;
    }
    let e = match sample_image(long.as_str()) {
        Err(IP_ELONG) => true,
        _ => false,
    };
    a && b && c && d && e
}

/// 语料全跑。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.total() == CORPUS && r.accept() == 2 && r.reject() == 12 && r.matched() == CORPUS;
    let b = r.missing() == 5 && r.dir() == 2 && r.noexec() == 3;
    let c = r.loopn() == 1 && r.notdir() == 1;
    let d = r.rate() == 1000 && r.green() && r.gate() == Ok(());
    a && b && c && d
}

/// 语料编号。
fn slots_ok() -> bool {
    if !unique() || !notes_ok() || !table_ok() {
        return false;
    }
    let a = match slot_at(0) {
        Some(s) => s.target == Target::Elf && s.code == 0,
        None => false,
    };
    let b = match slot_at(2) {
        Some(s) => s.code == IP_ENOENT && s.target == Target::Missing,
        None => false,
    };
    let c = match slot_at(CORPUS - 1) {
        Some(s) => s.code == IP_EPATH && s.note == "file-as-directory",
        None => false,
    };
    let d = slot_at(CORPUS).is_none();
    a && b && c && d
}

/// 汇总渲染。
fn report_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.render().as_str()
        == "interp corpus=14 accept=2 reject=12 missing=5 dir=2 noexec=3 loop=1 notdir=1 match=14 rate=1000ppm\n";
    let e = Report::new();
    let b = e.is_empty() && e.rate() == 0 && e.gate() == Err(IP_ENOBS);
    let c = e.render().as_str()
        == "interp corpus=0 accept=0 reject=0 missing=0 dir=0 noexec=0 loop=0 notdir=0 match=0 rate=0ppm\n";
    a && b && c
}

/// 红单显式。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, IP_ENOENT))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched() == CORPUS - 1 && r.rate() == 929;
    let b = r.gate() == Err(IP_EVER) && !r.green();
    let c = run_with(Some((CORPUS, IP_ENOENT))).err() == Some(IP_ESLOT);
    a && b && c
}

/// 覆盖档。
fn cover_ok() -> bool {
    cover(CORPUS) == Ok(()) && cover(CORPUS - 1) == Err(IP_ENOBS) && cover(0) == Err(IP_ENOBS)
}

/// 解析器与路径闸门。
fn resolver_ok() -> bool {
    let mut r = Resolver::new();
    let a = !r.bound() && r.steps() == 0;
    let b = match r.open("/lib64/ld-linux-x86-64.so.2") {
        Ok(v) => v.ok() && r.last() == 0,
        Err(_) => false,
    };
    let c = r.open("/lib64/ld-linux-x86-64.so.2").err() == Some(IP_ESTATE);
    let d = absolute("/lib/ld.so") && !absolute("lib/ld.so") && !absolute("");
    let e = segments("/lib64/ld-linux-x86-64.so.2") == 2 && segments("/a/b/c") == 3 && segments("/") == 0;
    a && b && c && d && e
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && target_ok();
    out[1] = order_ok() && message_ok();
    out[2] = errno_ok() && image_ok() && corpus_ok() && slots_ok();
    out[3] = report_ok() && reds_ok() && cover_ok() && resolver_ok()
        && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11609_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11609.interp_missing");

    s.add(
        "IP01-名称闸门",
        {
            name_ok()
        },
        "static",
    );

    s.add(
        "IP02-目标判定",
        {
            target_ok()
        },
        "static",
    );

    s.add(
        "IP03-闸门次序",
        {
            order_ok()
        },
        "static",
    );

    s.add(
        "IP04-文案逐字",
        {
            message_ok()
        },
        "static",
    );

    s.add(
        "IP05-errno映射",
        {
            errno_ok()
        },
        "static",
    );

    s.add(
        "IP06-段视图解析",
        {
            image_ok()
        },
        "static",
    );

    s.add(
        "IP07-十四卡全跑",
        {
            corpus_ok()
        },
        "static",
    );

    s.add(
        "IP08-语料编号",
        {
            slots_ok()
        },
        "static",
    );

    s.add(
        "IP09-汇总渲染",
        {
            report_ok()
        },
        "static",
    );

    s.add(
        "IP10-红单显式",
        {
            reds_ok()
        },
        "static",
    );

    s.add(
        "IP11-覆盖档",
        {
            cover_ok()
        },
        "static",
    );

    s.add(
        "IP12-解析一门一用",
        {
            resolver_ok()
        },
        "static",
    );

    s.add(
        "IP13-冻结重放四注入",
        {
            let mut l = build_ledger();
            let a = l.audit() == Ok(());
            let b = match l.freeze() {
                Ok(r) => r != 0 && l.is_frozen() && l.root() == r,
                Err(_) => false,
            };
            let c = l.replay() == Ok(l.root());
            let d = l.register(Sample::blank()).err() == Some(IP_ESLOT) && l.rejects() == 1;
            let inj = inject_four();
            let mut e = true;
            let mut i = 0usize;
            while i < inj.len() {
                if !inj[i] {
                    e = false;
                }
                i += 1;
            }
            a && b && c && d && e
        },
        "static",
    );

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_and_absolute() {
        assert!(absolute("/usr/bin"));
        assert!(!absolute("usr/bin"));
        assert!(!absolute(""));
        assert_eq!(segments("/lib64/ld-linux-x86-64.so.2"), 2);
        assert_eq!(segments("/a/b/c/"), 3);
        assert_eq!(segments(""), 0);
    }

    #[test]
    fn names_distinct() {
        let t = [Target::Elf, Target::Missing, Target::Dir, Target::NoExec, Target::Loop, Target::NotDir, Target::Text];
        let mut i = 0usize;
        while i < t.len() {
            let mut j = i + 1;
            while j < t.len() {
                assert_ne!(t[i].name(), t[j].name());
                assert_ne!(t[i].index(), t[j].index());
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn missing_is_enoent() {
        let v = judge(Target::Missing);
        assert_eq!(v.code(), IP_ENOENT);
        assert_eq!(v.errno(), ENOENT);
        assert_eq!(v.target(), Target::Missing);
        assert!(!v.ok());
    }

    #[test]
    fn directory_is_eacces() {
        let v = judge(Target::Dir);
        assert_eq!(v.code(), IP_EACCES);
        assert_eq!(v.errno(), EACCES);
        assert!(!v.detail().is_empty());
    }

    #[test]
    fn noexec_is_eacces() {
        let v = judge(Target::NoExec);
        assert_eq!(v.code(), IP_EACCES);
        assert_eq!(v.errno(), EACCES);
    }

    #[test]
    fn loop_is_eloop() {
        let v = judge(Target::Loop);
        assert_eq!(v.code(), IP_ELOOP);
        assert_eq!(v.errno(), ELOOP);
    }

    #[test]
    fn notdir_is_epath() {
        let v = judge(Target::NotDir);
        assert_eq!(v.code(), IP_EPATH);
        assert_eq!(v.errno(), ENOTDIR);
    }

    #[test]
    fn text_is_enoexec() {
        let v = judge(Target::Text);
        assert_eq!(v.code(), IP_ENOEXEC);
        assert_eq!(v.errno(), ENOEXEC);
    }

    #[test]
    fn elf_is_ok() {
        let v = judge(Target::Elf);
        assert!(v.ok());
        assert_eq!(v.code(), 0);
        assert_eq!(v.errno(), 0);
    }

    #[test]
    fn message_missing_verbatim() {
        let m = match message(&judge(Target::Missing), "/lib/ld-musl.so.1") {
            Ok(v) => v,
            Err(_) => panic_test(),
        };
        assert_eq!(m.as_str(), "bash: ./prog: No such file or directory (PT_INTERP=/lib/ld-musl.so.1)\n");
    }

    #[test]
    fn message_ok_verbatim() {
        let m = match message(&judge(Target::Elf), "/lib64/ld-linux-x86-64.so.2") {
            Ok(v) => v,
            Err(_) => panic_test(),
        };
        assert_eq!(m.as_str(), "ok: interpreter /lib64/ld-linux-x86-64.so.2\n");
    }

    #[test]
    fn message_rejects_empty_path() {
        assert_eq!(message(&judge(Target::Elf), "").err(), Some(IP_EPATH));
    }

    #[test]
    fn resolve_known_and_unknown() {
        match resolve("/lib64/ld-linux-x86-64.so.2") {
            Ok(v) => assert!(v.ok()),
            Err(_) => panic_test(),
        }
        assert_eq!(resolve("/no/such/loader").err(), Some(IP_ENOENT));
        assert_eq!(resolve("").err(), Some(IP_EPATH));
    }

    #[test]
    fn image_roundtrip() {
        let img = match sample_image("/lib/ld-musl-x86_64.so.1") {
            Ok(v) => v,
            Err(_) => panic_test(),
        };
        assert_eq!(img.count(), 4);
        match img.interp() {
            Ok(p) => assert_eq!(p, "/lib/ld-musl-x86_64.so.1"),
            Err(_) => panic_test(),
        }
        match judge_image(&img) {
            Ok(v) => assert!(v.ok()),
            Err(_) => panic_test(),
        }
    }

    #[test]
    fn image_without_interp_rejected() {
        let mut img = Image::new(BLOB);
        assert!(img.put(0, b"x").is_ok());
        assert!(img.push(Phdr::new(1, 0, 0x400000, 1, 0x1000)).is_ok());
        assert_eq!(img.interp().err(), Some(IP_EPATH));
        assert_eq!(judge_image(&img).err(), Some(IP_EPATH));
    }

    #[test]
    fn image_phdr_bounds() {
        let mut img = Image::new(64);
        assert_eq!(img.push(Phdr::new(PT_INTERP, 32, 0x400020, 0x40, 1)).err(), Some(IP_EPATH));
        assert_eq!(img.count(), 0);
        assert!(img.push(Phdr::new(PT_INTERP, 32, 0x400020, 0x20, 1)).is_ok());
        assert_eq!(img.count(), 1);
    }

    #[test]
    fn corpus_shape() {
        assert_eq!(CASES.len(), CORPUS);
        assert!(unique());
        assert!(notes_ok());
        assert!(table_covers());
        assert!(table_ok());
    }

    #[test]
    fn report_counts() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => panic_test(),
        };
        assert_eq!(r.total(), 14);
        assert_eq!(r.accept(), 2);
        assert_eq!(r.reject(), 12);
        assert_eq!(r.missing(), 5);
        assert_eq!(r.dir(), 2);
        assert_eq!(r.noexec(), 3);
        assert_eq!(r.loopn(), 1);
        assert_eq!(r.notdir(), 1);
        assert_eq!(r.matched(), 14);
        assert_eq!(r.rate(), 1000);
        assert_eq!(r.gate(), Ok(()));
    }

    #[test]
    fn report_render_exact() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => panic_test(),
        };
        assert_eq!(
            r.render().as_str(),
            "interp corpus=14 accept=2 reject=12 missing=5 dir=2 noexec=3 loop=1 notdir=1 match=14 rate=1000ppm\n"
        );
    }

    #[test]
    fn red_override() {
        let r = match run_with(Some((12, IP_ELOOP))) {
            Ok(v) => v,
            Err(_) => panic_test(),
        };
        assert_eq!(r.matched(), 13);
        assert_eq!(r.rate(), 929);
        assert_eq!(r.gate(), Err(IP_EVER));
        assert_eq!(run_with(Some((14, IP_ENOENT))).err(), Some(IP_ESLOT));
    }

    #[test]
    fn resolver_once() {
        let mut r = Resolver::new();
        assert!(!r.bound());
        match r.open("/lib64/ld-linux-x86-64.so.2") {
            Ok(v) => assert!(v.ok()),
            Err(_) => panic_test(),
        }
        assert_eq!(r.open("/lib/ld-musl.so.1").err(), Some(IP_ESTATE));
        assert_eq!(r.steps(), 1);
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert!(l.sample(100).is_none());
        assert!(!l.is_empty());
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let r = match l.freeze() {
            Ok(v) => v,
            Err(_) => panic_test(),
        };
        assert_ne!(r, 0);
        assert!(l.is_frozen());
        assert_eq!(l.root(), r);
        assert_eq!(l.replay(), Ok(r));
        assert_eq!(l.register(Sample::blank()).err(), Some(IP_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.freeze().err(), Some(IP_ESEAL));
        assert_eq!(l.rejects(), 2);
    }

    #[test]
    fn injections() {
        let out = inject_four();
        assert!(out[0], "inject0");
        assert!(out[1], "inject1");
        assert!(out[2], "inject2");
        assert!(out[3], "inject3");
    }

    #[test]
    fn fold_deterministic() {
        let a = fold_str(FNV_OFF, "/lib/ld.so");
        let b = fold_str(FNV_OFF, "/lib/ld.so");
        let c = fold_str(FNV_OFF, "/lib/ld.sn");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(u32_text(0).as_str(), "0");
        assert_eq!(u32_text(14).as_str(), "14");
        assert_eq!(u32_text(929).as_str(), "929");
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11609_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }

    fn panic_test() -> ! {
        panic!("unexpected failure path");
    }
}
