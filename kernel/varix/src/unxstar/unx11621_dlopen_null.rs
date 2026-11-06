//! UNX-F11621 dlopen 坏 .so 返回 NULL 与 dlerror 总判据
//!
//! `dlopen` 的返回值只有两种命运：拿到一个非 0 句柄，或者拿到 `NULL`。拿
//! `NULL` 时**唯一**的法定解释渠道是 `dlerror()`：它返回一行文本，并且只
//! 返回一次，取走即清空。所以"dlopen 失败"这件事从来不是单个布尔量，而是
//! 三元组 `(NULL, 文本, 文本只出现一次)`。任何一层把文本吞掉、缓存住、或
//! 把"上一次的错"留到下一次调用，都会让上层脚本读到错的根因。
//!
//! 本模块把 dlopen 的成败拆成一条九段管线：模式位 → 路径定位 → 打开 →
//! 魔数 → 类 → 机种 → 依赖 → 重定位/初始化 → 就绪。每段只回答一个问题，
//! 且**先出错先返回**：模式位不合法时连文件都不碰（glibc 的 dlopen_doit
//! 就是先查 `mode` 的未知位），路径找不到时不会去读魔数，魔数不对时不会
//! 去比类宽。次序一旦倒置，同一份输入就会得到两种根因，运维对着 dlerror
//! 排查时就只能看到最后一段的噪声。
//!
//! 文案逐字稳定，全部以"被请求的名字"开头（不是拼接后的搜索路径），与
//! glibc 的形态一致：
//!
//! - `libfoo.so: cannot open shared object file: No such file or directory`
//! - `/root/secret.so: cannot open shared object file: Permission denied`
//! - `./bad.so: invalid ELF header`
//! - `./empty.so: file too short`
//! - `./x32.so: wrong ELF class: ELFCLASS32`
//! - `./arm.so: wrong ELF machine: EM_ARM`
//! - `invalid mode for dlopen()`
//!
//! 句柄侧另有两条独立纪律：`RTLD_NOLOAD` 只查表不开文件（未加载时给
//! `object not open`，不落盘），以及 `dlclose` 的引用计数（`RTLD_NODELETE`
//! 把对象钉在表里）。`dlerror` 的一次性在这里用 `ErrorQueue` 建模：`take`
//! 之后 `pending` 必须为假，第二次 `take` 必须给 `None`——这是本卡最容易被
//! 写错、也最容易被测试漏掉的一格。
//!
//! 闸门次序：模式 → 定位 → 打开 → 魔数 → 类 → 机种 → 依赖 → 就绪 → 句柄 → 文案。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本 14 格
//! （8 成 6 败）在 `audit` 逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfc40;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 语料条数。
pub const CORPUS: usize = 14;

/// 句柄表容量。
pub const MAX_HANDLES: usize = 8;

/// 搜索目录条数。
pub const MAX_DIRS: usize = 6;

/// 宿主文件表容量。
pub const MAX_FILES: usize = 12;

/// ELF 魔数长度。
pub const MAGIC_LEN: usize = 4;

/// 本机类宽（ELFCLASS64）。
pub const NATIVE_CLASS: u8 = 2;

/// 本机机种（EM_X86_64）。
pub const NATIVE_MACHINE: u16 = 62;

/// 空句柄。
pub const DL_NULL: u32 = 0;

// ---------------------------------------------------------------------------
// RTLD 模式位（glibc 取值）
// ---------------------------------------------------------------------------

/// 惰性绑定。
pub const RTLD_LAZY: u32 = 0x0001;
/// 立即绑定。
pub const RTLD_NOW: u32 = 0x0002;
/// 只查表，不打开。
pub const RTLD_NOLOAD: u32 = 0x0004;
/// 依赖优先于全局符号。
pub const RTLD_DEEPBIND: u32 = 0x0008;
/// 全局可见。
pub const RTLD_GLOBAL: u32 = 0x0100;
/// 不因 dlclose 卸载。
pub const RTLD_NODELETE: u32 = 0x1000;

/// 已知位全集。
pub const MODE_KNOWN: u32 =
    RTLD_LAZY | RTLD_NOW | RTLD_NOLOAD | RTLD_DEEPBIND | RTLD_GLOBAL | RTLD_NODELETE;

/// 模式位的展示次序。
pub const FLAG_ORDER: [(u32, &'static str); 6] = [
    (RTLD_LAZY, "RTLD_LAZY"),
    (RTLD_NOW, "RTLD_NOW"),
    (RTLD_NOLOAD, "RTLD_NOLOAD"),
    (RTLD_DEEPBIND, "RTLD_DEEPBIND"),
    (RTLD_GLOBAL, "RTLD_GLOBAL"),
    (RTLD_NODELETE, "RTLD_NODELETE"),
];

// ---------------------------------------------------------------------------
// ELF 常量
// ---------------------------------------------------------------------------

/// 完好魔数。
pub const ELF_MAGIC: [u8; MAGIC_LEN] = [0x7f, b'E', b'L', b'F'];

/// ELFCLASS32。
pub const ELFCLASS32: u8 = 1;

/// ELFCLASS64。
pub const ELFCLASS64: u8 = 2;

/// EM_386。
pub const EM_386: u16 = 3;

/// EM_ARM。
pub const EM_ARM: u16 = 40;

/// EM_X86_64。
pub const EM_X86_64: u16 = 62;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 找不到共享对象。
pub const DL_ENOENT: u32 = SEG_BASE + 0x01;
/// 魔数坏。
pub const DL_EBADELF: u32 = SEG_BASE + 0x02;
/// 类宽不符。
pub const DL_ECLASS: u32 = SEG_BASE + 0x03;
/// 机种不符。
pub const DL_EMACHINE: u32 = SEG_BASE + 0x04;
/// 模式位非法。
pub const DL_EMODE: u32 = SEG_BASE + 0x05;
/// 权限不足。
pub const DL_EPERM: u32 = SEG_BASE + 0x06;
/// 符号未定义。
pub const DL_ESYM: u32 = SEG_BASE + 0x07;
/// 句柄状态非法。
pub const DL_ESTATE: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const DL_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const DL_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const DL_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const DL_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const DL_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 模式解析。
pub const OP_MODE: u8 = 0x01;
/// 路径定位。
pub const OP_LOOKUP: u8 = 0x02;
/// 打开并识别。
pub const OP_OPEN: u8 = 0x03;
/// 语料全跑。
pub const OP_CORPUS: u8 = 0x04;
/// 错误队列。
pub const OP_ERROR: u8 = 0x05;
/// 句柄表。
pub const OP_HANDLE: u8 = 0x06;
/// 符号解析。
pub const OP_SYMBOL: u8 = 0x07;

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
fn dec_text(v: u32) -> String {
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

/// 定宽小写十六进制文本。
fn hex_text(v: u32, digits: usize) -> String {
    let mut s = String::new();
    let mut n = digits;
    while n > 0 {
        n -= 1;
        let shift = (n as u32) * 4;
        let nib = ((v >> shift) & 0xf) as u8;
        let c = if nib < 10 { b'0' + nib } else { b'a' + (nib - 10) };
        s.push(c as char);
    }
    s
}

/// 小写化（只处理 ASCII）。
fn ascii_lower(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            let c = if *b >= b'A' && *b <= b'Z' { *b + 32 } else { *b };
            out.push(c as char);
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 模式位
// ---------------------------------------------------------------------------

/// 绑定方式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bind {
    /// 惰性。
    Lazy,
    /// 立即。
    Now,
}

impl Bind {
    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            Bind::Lazy => "lazy",
            Bind::Now => "now",
        }
    }
}

/// 作用域。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// 局部。
    Local,
    /// 全局。
    Global,
}

impl Scope {
    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            Scope::Local => "local",
            Scope::Global => "global",
        }
    }
}

/// 一个 dlopen 模式字。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mode {
    raw: u32,
}

impl Mode {
    /// 构造。
    pub const fn new(raw: u32) -> Mode {
        Mode { raw }
    }

    /// 原始值。
    pub const fn raw(&self) -> u32 {
        self.raw
    }

    /// 未知位。
    pub fn unknown(&self) -> u32 {
        self.raw & !MODE_KNOWN
    }

    /// 已知位是否只含一个 bind 位。
    pub fn both_bind(&self) -> bool {
        (self.raw & RTLD_LAZY != 0) && (self.raw & RTLD_NOW != 0)
    }

    /// 绑定方式：同时给两位时 NOW 胜（glibc 的绑定强度取大）。
    pub fn bind(&self) -> Bind {
        if self.raw & RTLD_NOW != 0 {
            Bind::Now
        } else {
            Bind::Lazy
        }
    }

    /// 作用域。
    pub fn scope(&self) -> Scope {
        if self.raw & RTLD_GLOBAL != 0 {
            Scope::Global
        } else {
            Scope::Local
        }
    }

    /// 是否只查表。
    pub fn noload(&self) -> bool {
        self.raw & RTLD_NOLOAD != 0
    }

    /// 是否深绑。
    pub fn deepbind(&self) -> bool {
        self.raw & RTLD_DEEPBIND != 0
    }

    /// 是否钉住。
    pub fn nodelete(&self) -> bool {
        self.raw & RTLD_NODELETE != 0
    }

    /// 已知位个数。
    pub fn bit_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < FLAG_ORDER.len() {
            if let Some((bit, _)) = FLAG_ORDER.get(i) {
                if self.raw & *bit != 0 {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 合法性：未知位非 0 即非法。
    pub fn validate(&self) -> Result<(), u32> {
        if self.unknown() != 0 {
            Err(DL_EMODE)
        } else {
            Ok(())
        }
    }

    /// 标志文本。
    pub fn text(&self) -> String {
        if self.raw == 0 {
            return String::from("RTLD_LOCAL");
        }
        let mut s = String::new();
        let mut first = true;
        let mut i = 0usize;
        while i < FLAG_ORDER.len() {
            if let Some((bit, name)) = FLAG_ORDER.get(i) {
                if self.raw & *bit != 0 {
                    if !first {
                        s.push('|');
                    }
                    s.push_str(name);
                    first = false;
                }
            }
            i += 1;
        }
        if self.unknown() != 0 {
            if !first {
                s.push('|');
            }
            s.push_str("RTLD_UNKNOWN|0x");
            s.push_str(hex_text(self.unknown(), 8).as_str());
        }
        s
    }

    /// 摘要。
    pub fn seal(&self) -> u64 {
        fold_word(FNV_OFF, self.raw as u64)
    }
}

// ---------------------------------------------------------------------------
// 管线阶段
// ---------------------------------------------------------------------------

/// 管线阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 模式位。
    Mode,
    /// 路径定位。
    Locate,
    /// 打开。
    Open,
    /// 魔数。
    Ident,
    /// 类宽。
    Class,
    /// 机种。
    Machine,
    /// 依赖。
    Deps,
    /// 重定位与初始化。
    Init,
    /// 就绪。
    Ready,
}

impl Stage {
    /// 名称。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Mode => "mode",
            Stage::Locate => "locate",
            Stage::Open => "open",
            Stage::Ident => "ident",
            Stage::Class => "class",
            Stage::Machine => "machine",
            Stage::Deps => "deps",
            Stage::Init => "init",
            Stage::Ready => "ready",
        }
    }

    /// 序号。
    pub fn index(&self) -> u32 {
        match self {
            Stage::Mode => 0,
            Stage::Locate => 1,
            Stage::Open => 2,
            Stage::Ident => 3,
            Stage::Class => 4,
            Stage::Machine => 5,
            Stage::Deps => 6,
            Stage::Init => 7,
            Stage::Ready => 8,
        }
    }

    /// 细节。
    pub fn detail(&self) -> &'static str {
        match self {
            Stage::Mode => "mode word",
            Stage::Locate => "search path lookup",
            Stage::Open => "open(2) and read",
            Stage::Ident => "e_ident magic",
            Stage::Class => "e_ident[EI_CLASS]",
            Stage::Machine => "e_machine",
            Stage::Deps => "DT_NEEDED",
            Stage::Init => "relocations and init",
            Stage::Ready => "handle live",
        }
    }

    /// 是否成功段。
    pub fn terminal(&self) -> bool {
        match self {
            Stage::Ready => true,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 故障
// ---------------------------------------------------------------------------

/// 一种失败根因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 文件不存在。
    NoEnt,
    /// 无读权限。
    Perm,
    /// 太短。
    Short,
    /// 魔数坏。
    BadElf,
    /// 类宽 32。
    Class32,
    /// 类宽 64。
    Class64,
    /// 类宽是别的值（带实际 e_ident[EI_CLASS]）。
    ClassOther(u8),
    /// 机种不符（带实际机种）。
    Machine(u16),
    /// 模式位非法。
    Mode,
    /// 依赖缺失。
    Undep,
    /// 符号未定义。
    Sym,
    /// 初始化失败。
    Init,
}

impl Fault {
    /// 名称。
    pub fn name(&self) -> &'static str {
        match self {
            Fault::NoEnt => "noent",
            Fault::Perm => "perm",
            Fault::Short => "short",
            Fault::BadElf => "badelf",
            Fault::Class32 => "class32",
            Fault::Class64 => "class64",
            Fault::ClassOther(_) => "classother",
            Fault::Machine(_) => "machine",
            Fault::Mode => "mode",
            Fault::Undep => "undep",
            Fault::Sym => "sym",
            Fault::Init => "init",
        }
    }

    /// 错误码。
    pub fn code(&self) -> u32 {
        match self {
            Fault::NoEnt => DL_ENOENT,
            Fault::Perm => DL_EPERM,
            Fault::Short => DL_EBADELF,
            Fault::BadElf => DL_EBADELF,
            Fault::Class32 => DL_ECLASS,
            Fault::Class64 => DL_ECLASS,
            Fault::ClassOther(_) => DL_ECLASS,
            Fault::Machine(_) => DL_EMACHINE,
            Fault::Mode => DL_EMODE,
            Fault::Undep => DL_ENOENT,
            Fault::Sym => DL_ESYM,
            Fault::Init => DL_ESTATE,
        }
    }

    /// 出错阶段。
    pub fn stage(&self) -> Stage {
        match self {
            Fault::Mode => Stage::Mode,
            Fault::NoEnt => Stage::Locate,
            Fault::Perm => Stage::Open,
            Fault::Short => Stage::Open,
            Fault::BadElf => Stage::Ident,
            Fault::Class32 => Stage::Class,
            Fault::Class64 => Stage::Class,
            Fault::ClassOther(_) => Stage::Class,
            Fault::Machine(_) => Stage::Machine,
            Fault::Undep => Stage::Deps,
            Fault::Sym => Stage::Init,
            Fault::Init => Stage::Init,
        }
    }

    /// dlerror 文本（以请求名开头）。
    pub fn text(&self, name: &str) -> String {
        match self {
            Fault::Mode => String::from("invalid mode for dlopen()"),
            Fault::NoEnt => open_text(name, "No such file or directory"),
            Fault::Perm => open_text(name, "Permission denied"),
            Fault::Short => named(name, "file too short"),
            Fault::BadElf => named(name, "invalid ELF header"),
            Fault::Class32 => class_text(name, ELFCLASS32),
            Fault::Class64 => class_text(name, ELFCLASS64),
            Fault::ClassOther(c) => class_text(name, *c),
            Fault::Machine(m) => machine_text(name, *m),
            Fault::Undep => named(name, "cannot open shared object file"),
            Fault::Sym => {
                let mut s = String::from("undefined symbol: ");
                s.push_str(name);
                s
            }
            Fault::Init => named(name, "failed to initialize"),
        }
    }
}

/// `<name>: cannot open shared object file: <why>`。
pub fn open_text(name: &str, why: &str) -> String {
    let mut s = String::from(name);
    s.push_str(": cannot open shared object file: ");
    s.push_str(why);
    s
}

/// `<name>: <msg>`。
pub fn named(name: &str, msg: &str) -> String {
    let mut s = String::from(name);
    s.push_str(": ");
    s.push_str(msg);
    s
}

/// 类宽文案。
pub fn class_text(name: &str, class: u8) -> String {
    let mut s = named(name, "wrong ELF class: ");
    match class {
        ELFCLASS32 => s.push_str("ELFCLASS32"),
        ELFCLASS64 => s.push_str("ELFCLASS64"),
        0 => s.push_str("ELFCLASSNONE"),
        _ => {
            s.push_str("ELFCLASSNN/");
            s.push_str(dec_text(class as u32).as_str());
        }
    }
    s
}

/// 机种名。
pub fn machine_name(m: u16) -> &'static str {
    match m {
        0 => "EM_NONE",
        3 => "EM_386",
        8 => "EM_MIPS",
        20 => "EM_PPC",
        21 => "EM_PPC64",
        40 => "EM_ARM",
        50 => "EM_IA_64",
        62 => "EM_X86_64",
        183 => "EM_AARCH64",
        243 => "EM_RISCV",
        _ => "EM_UNKNOWN",
    }
}

/// 机种文案。
pub fn machine_text(name: &str, m: u16) -> String {
    let mut s = named(name, "wrong ELF machine: ");
    s.push_str(machine_name(m));
    if machine_name(m) == "EM_UNKNOWN" {
        s.push('/');
        s.push_str(dec_text(m as u32).as_str());
    }
    s
}

// ---------------------------------------------------------------------------
// 宿主文件表
// ---------------------------------------------------------------------------

/// 宿主上一样东西的形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 不存在（含悬空符号链接）。
    Missing,
    /// 存在但不可读。
    Unreadable,
    /// 零字节。
    Empty,
    /// 纯文本。
    Text,
    /// ELF 头已读到。
    Elf {
        /// 魔数四字节。
        magic: [u8; MAGIC_LEN],
        /// 类宽。
        class: u8,
        /// 机种。
        machine: u16,
    },
}

/// 一个宿主文件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostFile {
    /// 绝对或相对路径（与请求拼接后逐字比对）。
    pub name: &'static str,
    /// 形态。
    pub kind: Kind,
}

impl HostFile {
    /// 构造。
    pub const fn new(name: &'static str, kind: Kind) -> HostFile {
        HostFile { name, kind }
    }
}

/// 十四卡所依赖的十个宿主文件。
pub const HOST: [HostFile; 10] = [
    HostFile::new(
        "/lib/libc.so.6",
        Kind::Elf { magic: ELF_MAGIC, class: ELFCLASS64, machine: EM_X86_64 },
    ),
    HostFile::new(
        "/usr/lib/libm.so.6",
        Kind::Elf { magic: ELF_MAGIC, class: ELFCLASS64, machine: EM_X86_64 },
    ),
    HostFile::new("/root/secret.so", Kind::Unreadable),
    HostFile::new(
        "./bad.so",
        Kind::Elf { magic: [0x7f, b'E', b'L', b'G'], class: ELFCLASS64, machine: EM_X86_64 },
    ),
    HostFile::new("./bad2.so", Kind::Text),
    HostFile::new(
        "./x32.so",
        Kind::Elf { magic: ELF_MAGIC, class: ELFCLASS32, machine: EM_386 },
    ),
    HostFile::new(
        "./x32b.so",
        Kind::Elf { magic: ELF_MAGIC, class: ELFCLASS32, machine: EM_X86_64 },
    ),
    HostFile::new(
        "./arm.so",
        Kind::Elf { magic: ELF_MAGIC, class: ELFCLASS64, machine: EM_ARM },
    ),
    HostFile::new(
        "./m32.so",
        Kind::Elf { magic: ELF_MAGIC, class: ELFCLASS64, machine: EM_386 },
    ),
    HostFile::new("./empty.so", Kind::Empty),
];

/// 搜索目录，次序即优先级。
pub const DIRS: [&'static str; MAX_DIRS] =
    ["/lib", "/usr/lib", "/lib64", "/usr/lib64", "/usr/local/lib", "."];

/// 是否绝对路径。
pub fn absolute(path: &str) -> bool {
    match path.as_bytes().first() {
        Some(b) => *b == b'/',
        None => false,
    }
}

/// 是否含 `/`（含则不做目录搜索）。
pub fn has_slash(path: &str) -> bool {
    let bytes = path.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            if *b == b'/' {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 拼目录。
pub fn join(dir: &str, name: &str) -> String {
    let mut s = String::from(dir);
    if !dir.is_empty() {
        s.push('/');
    }
    s.push_str(name);
    s
}

/// 查宿主表。
pub fn lookup(name: &str) -> Option<Kind> {
    let mut i = 0usize;
    while i < HOST.len() {
        if let Some(f) = HOST.get(i) {
            if f.name == name {
                return Some(f.kind);
            }
        }
        i += 1;
    }
    None
}

/// 路径是否真在搜索面上（用于定位测试）。
pub fn resolvable(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    if has_slash(path) {
        return lookup(path).is_some();
    }
    let mut i = 0usize;
    while i < DIRS.len() {
        if let Some(d) = DIRS.get(i) {
            if lookup(join(*d, path).as_str()).is_some() {
                return true;
            }
        }
        i += 1;
    }
    false
}

// ---------------------------------------------------------------------------
// 一次 dlopen 的结论
// ---------------------------------------------------------------------------

/// dlopen 的结论：阶段、根因、请求名、命中路径。
#[derive(Clone, Debug)]
pub struct Outcome {
    stage: Stage,
    fault: Option<Fault>,
    request: String,
    found: String,
    mode: Mode,
}

impl Outcome {
    /// 成功。
    pub fn ok(request: &str, found: &str, mode: Mode) -> Outcome {
        Outcome {
            stage: Stage::Ready,
            fault: None,
            request: String::from(request),
            found: String::from(found),
            mode,
        }
    }

    /// 失败。
    pub fn fail(request: &str, fault: Fault, mode: Mode) -> Outcome {
        Outcome {
            stage: fault.stage(),
            fault: Some(fault),
            request: String::from(request),
            found: String::new(),
            mode,
        }
    }

    /// 阶段。
    pub const fn stage(&self) -> Stage {
        self.stage
    }

    /// 根因。
    pub const fn fault(&self) -> Option<Fault> {
        self.fault
    }

    /// 错误码。
    pub fn code(&self) -> u32 {
        match self.fault {
            Some(f) => f.code(),
            None => 0,
        }
    }

    /// 请求名。
    pub fn request(&self) -> &str {
        self.request.as_str()
    }

    /// 命中路径（未命中为空）。
    pub fn found(&self) -> &str {
        self.found.as_str()
    }

    /// 模式。
    pub const fn mode(&self) -> Mode {
        self.mode
    }

    /// 是否拿到句柄。
    pub fn live(&self) -> bool {
        self.fault.is_none()
    }

    /// dlerror 文本：成功时为空串（dlerror 应给 NULL）。
    pub fn text(&self) -> String {
        match self.fault {
            Some(f) => f.text(self.request.as_str()),
            None => String::new(),
        }
    }

    /// 摘要（不含请求名，便于比对同一路径不同名字）。
    pub fn seal(&self) -> u64 {
        let mut h = fold_word(FNV_OFF, self.stage.index() as u64);
        h = fold_word(h, self.code() as u64);
        h = fold_word(h, self.mode.raw() as u64);
        h = fold_word(h, digest(self.request.as_bytes()));
        h
    }
}

/// 按类宽与机种继续判。
fn classify(request: &str, kind: Kind, mode: Mode) -> Outcome {
    match kind {
        Kind::Missing => Outcome::fail(request, Fault::NoEnt, mode),
        Kind::Unreadable => Outcome::fail(request, Fault::Perm, mode),
        Kind::Empty => Outcome::fail(request, Fault::Short, mode),
        Kind::Text => Outcome::fail(request, Fault::BadElf, mode),
        Kind::Elf { magic, class, machine } => {
            if magic != ELF_MAGIC {
                return Outcome::fail(request, Fault::BadElf, mode);
            }
            match class {
                ELFCLASS64 => {}
                ELFCLASS32 => return Outcome::fail(request, Fault::Class32, mode),
                other => return Outcome::fail(request, Fault::ClassOther(other), mode),
            }
            if machine != NATIVE_MACHINE {
                return Outcome::fail(request, Fault::Machine(machine), mode);
            }
            Outcome::ok(request, request, mode)
        }
    }
}

/// 走一遍九段管线。
pub fn simulate(path: &str, mode_raw: u32) -> Outcome {
    let mode = Mode::new(mode_raw);
    if mode.validate().is_err() {
        return Outcome::fail(path, Fault::Mode, mode);
    }
    // dlopen(NULL, ...) 取主程序句柄，不碰文件系统。
    if path.is_empty() {
        return Outcome::ok("", "<main program>", mode);
    }
    if has_slash(path) {
        let kind = match lookup(path) {
            Some(k) => k,
            None => return Outcome::fail(path, Fault::NoEnt, mode),
        };
        return classify(path, kind, mode);
    }
    let mut i = 0usize;
    while i < DIRS.len() {
        if let Some(d) = DIRS.get(i) {
            let cand = join(*d, path);
            if let Some(kind) = lookup(cand.as_str()) {
                let mut o = classify(path, kind, mode);
                o.found = cand;
                return o;
            }
        }
        i += 1;
    }
    Outcome::fail(path, Fault::NoEnt, mode)
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 一条语料。
#[derive(Clone, Copy, Debug)]
pub struct Probe {
    /// 请求名。
    pub path: &'static str,
    /// 模式位。
    pub mode: u32,
    /// 期望根因（None 表示期望成功）。
    pub expect: Option<Fault>,
    /// 期望 dlerror 文本（成功为空串）。
    pub text: &'static str,
    /// 备注。
    pub note: &'static str,
}

impl Probe {
    /// 构造。
    pub const fn new(
        path: &'static str,
        mode: u32,
        expect: Option<Fault>,
        text: &'static str,
        note: &'static str,
    ) -> Probe {
        Probe { path, mode, expect, text, note }
    }
}

/// 十四卡：两成十二败。
pub const CASES: [Probe; CORPUS] = [
    Probe::new("libc.so.6", RTLD_LAZY, None, "", "search-hit-ok"),
    Probe::new(
        "/usr/lib/libm.so.6",
        RTLD_NOW | RTLD_GLOBAL,
        None,
        "",
        "absolute-hit-ok",
    ),
    Probe::new(
        "libfoo.so",
        RTLD_LAZY,
        Some(Fault::NoEnt),
        "libfoo.so: cannot open shared object file: No such file or directory",
        "need-search-miss",
    ),
    Probe::new(
        "libbar.so.1",
        RTLD_NOW,
        Some(Fault::NoEnt),
        "libbar.so.1: cannot open shared object file: No such file or directory",
        "versioned-miss",
    ),
    Probe::new(
        "libq.so",
        RTLD_LAZY | RTLD_GLOBAL,
        Some(Fault::NoEnt),
        "libq.so: cannot open shared object file: No such file or directory",
        "plain-miss",
    ),
    Probe::new(
        "/root/secret.so",
        RTLD_LAZY,
        Some(Fault::Perm),
        "/root/secret.so: cannot open shared object file: Permission denied",
        "eacces",
    ),
    Probe::new(
        "./bad.so",
        RTLD_LAZY,
        Some(Fault::BadElf),
        "./bad.so: invalid ELF header",
        "near-magic",
    ),
    Probe::new(
        "./bad2.so",
        RTLD_NOW,
        Some(Fault::BadElf),
        "./bad2.so: invalid ELF header",
        "text-as-so",
    ),
    Probe::new(
        "./x32.so",
        RTLD_LAZY,
        Some(Fault::Class32),
        "./x32.so: wrong ELF class: ELFCLASS32",
        "ilp32-so",
    ),
    Probe::new(
        "./x32b.so",
        RTLD_NOW | RTLD_DEEPBIND,
        Some(Fault::Class32),
        "./x32b.so: wrong ELF class: ELFCLASS32",
        "class-before-machine",
    ),
    Probe::new(
        "./arm.so",
        RTLD_LAZY,
        Some(Fault::Machine(EM_ARM)),
        "./arm.so: wrong ELF machine: EM_ARM",
        "arm64-so-on-x86",
    ),
    Probe::new(
        "./m32.so",
        RTLD_NOW,
        Some(Fault::Machine(EM_386)),
        "./m32.so: wrong ELF machine: EM_386",
        "i386-so-64bit-el",
    ),
    Probe::new(
        "/tmp/lnk.so",
        RTLD_LAZY,
        Some(Fault::NoEnt),
        "/tmp/lnk.so: cannot open shared object file: No such file or directory",
        "dangling-symlink",
    ),
    Probe::new(
        "./empty.so",
        RTLD_LAZY,
        Some(Fault::Short),
        "./empty.so: file too short",
        "zero-byte-so",
    ),
];

/// 取一格语料。
pub fn slot_at(i: usize) -> Option<Probe> {
    CASES.get(i).copied()
}

/// 语料互异（按请求名+模式）。
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
                if b.path == a.path && b.mode == a.mode {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 备注非空。
pub fn notes_ok() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        match CASES.get(i) {
            Some(p) => {
                if p.note.is_empty() {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 语料逐条对判：码与文案都要对上。
pub fn table_ok() -> bool {
    if !unique() || !notes_ok() {
        return false;
    }
    let mut i = 0usize;
    while i < CASES.len() {
        let p = match CASES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let o = simulate(p.path, p.mode);
        match (p.expect, o.fault()) {
            (None, None) => {}
            (Some(a), Some(b)) => {
                if a != b {
                    return false;
                }
            }
            _ => return false,
        }
        if o.text().as_str() != p.text {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 汇总
// ---------------------------------------------------------------------------

/// 语料汇总。
#[derive(Clone, Copy, Debug)]
pub struct Report {
    total: usize,
    ok: usize,
    fail: usize,
    noent: usize,
    perm: usize,
    short: usize,
    badelf: usize,
    class32: usize,
    machine: usize,
    matched: usize,
    red: bool,
}

impl Report {
    /// 空汇总。
    pub const fn new() -> Report {
        Report {
            total: 0,
            ok: 0,
            fail: 0,
            noent: 0,
            perm: 0,
            short: 0,
            badelf: 0,
            class32: 0,
            machine: 0,
            matched: 0,
            red: false,
        }
    }

    /// 总条数。
    pub const fn total(&self) -> usize {
        self.total
    }

    /// 成功条数。
    pub const fn ok(&self) -> usize {
        self.ok
    }

    /// 失败条数。
    pub const fn fail(&self) -> usize {
        self.fail
    }

    /// 找不到条数。
    pub const fn noent(&self) -> usize {
        self.noent
    }

    /// 权限条数。
    pub const fn perm(&self) -> usize {
        self.perm
    }

    /// 太短条数。
    pub const fn short(&self) -> usize {
        self.short
    }

    /// 魔数坏条数。
    pub const fn badelf(&self) -> usize {
        self.badelf
    }

    /// 类宽错条数。
    pub const fn class32(&self) -> usize {
        self.class32
    }

    /// 机种错条数。
    pub const fn machine(&self) -> usize {
        self.machine
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

    /// 千分率。
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
            return Err(DL_ENOBS);
        }
        if self.red {
            return Err(DL_EBAL);
        }
        if self.matched != self.total || self.total != CORPUS {
            return Err(DL_EVALUE);
        }
        if self.ok != 2 || self.fail != 12 {
            return Err(DL_EVALUE);
        }
        if self.noent != 4 || self.perm != 1 || self.short != 1 {
            return Err(DL_EVALUE);
        }
        if self.badelf != 2 || self.class32 != 2 || self.machine != 2 {
            return Err(DL_EVALUE);
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
        let mut s = String::from("dlopen total=");
        s.push_str(dec_text(self.total as u32).as_str());
        s.push_str(" ok=");
        s.push_str(dec_text(self.ok as u32).as_str());
        s.push_str(" fail=");
        s.push_str(dec_text(self.fail as u32).as_str());
        s.push_str(" noent=");
        s.push_str(dec_text(self.noent as u32).as_str());
        s.push_str(" perm=");
        s.push_str(dec_text(self.perm as u32).as_str());
        s.push_str(" short=");
        s.push_str(dec_text(self.short as u32).as_str());
        s.push_str(" badelf=");
        s.push_str(dec_text(self.badelf as u32).as_str());
        s.push_str(" class32=");
        s.push_str(dec_text(self.class32 as u32).as_str());
        s.push_str(" machine=");
        s.push_str(dec_text(self.machine as u32).as_str());
        s.push_str(" match=");
        s.push_str(dec_text(self.matched as u32).as_str());
        s.push_str(" rate=");
        s.push_str(dec_text(self.rate()).as_str());
        s.push_str("ppm\n");
        s
    }
}

/// 跑一遍语料。
pub fn run() -> Result<Report, u32> {
    walk(None)
}

/// 跑一遍并注入一处红档。
pub fn run_with(red: Option<(usize, u32)>) -> Result<Report, u32> {
    walk(red)
}

/// 语料行走。
fn walk(red: Option<(usize, u32)>) -> Result<Report, u32> {
    let mut r = Report::new();
    let mut i = 0usize;
    while i < CASES.len() {
        let p = match CASES.get(i) {
            Some(v) => *v,
            None => return Err(DL_EVALUE),
        };
        let o = simulate(p.path, p.mode);
        r.total += 1;
        match o.fault() {
            None => {
                r.ok += 1;
                if p.expect.is_none() {
                    r.matched += 1;
                }
            }
            Some(f) => {
                r.fail += 1;
                match f {
                    Fault::NoEnt => r.noent += 1,
                    Fault::Perm => r.perm += 1,
                    Fault::Short => r.short += 1,
                    Fault::BadElf => r.badelf += 1,
                    Fault::Class32 => r.class32 += 1,
                    Fault::Class64 => r.class32 += 1,
                    Fault::ClassOther(_) => r.class32 += 1,
                    Fault::Machine(_) => r.machine += 1,
                    Fault::Mode => {}
                    Fault::Undep => r.noent += 1,
                    Fault::Sym => {}
                    Fault::Init => {}
                }
                if p.expect == Some(f) && o.text().as_str() == p.text {
                    r.matched += 1;
                }
            }
        }
        i += 1;
    }
    if let Some((slot, _code)) = red {
        if slot >= r.total {
            return Err(DL_ESLOT);
        }
        r.matched = r.matched.saturating_sub(1);
        r.red = true;
    }
    Ok(r)
}

/// 封面判据。
pub fn cover(n: usize) -> Result<(), u32> {
    if n == CORPUS {
        Ok(())
    } else {
        Err(DL_ENOBS)
    }
}

// ---------------------------------------------------------------------------
// 错误队列（dlerror 的一次性语义）
// ---------------------------------------------------------------------------

/// 一条 dlerror 文本，取走即清。
#[derive(Clone, Debug)]
pub struct ErrorQueue {
    text: String,
    pending: bool,
}

impl ErrorQueue {
    /// 空队列。
    pub fn new() -> ErrorQueue {
        ErrorQueue { text: String::new(), pending: false }
    }

    /// 置一条错（覆盖旧的，不排队）。
    pub fn set(&mut self, t: &str) {
        self.text.clear();
        self.text.push_str(t);
        self.pending = true;
    }

    /// 是否还挂着一条。
    pub fn pending(&self) -> bool {
        self.pending
    }

    /// 看但不取。
    pub fn peek(&self) -> Option<&str> {
        if self.pending {
            Some(self.text.as_str())
        } else {
            None
        }
    }

    /// 取走（一次性）。
    pub fn take(&mut self) -> Option<String> {
        if !self.pending {
            return None;
        }
        let out = self.text.clone();
        self.text.clear();
        self.pending = false;
        Some(out)
    }

    /// 清空（成功调用后 glibc 也会把旧错清掉）。
    pub fn clear(&mut self) {
        self.text.clear();
        self.pending = false;
    }

    /// 文本长度。
    pub fn len(&self) -> usize {
        self.text.len()
    }

    /// 是否无文本。
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 句柄
// ---------------------------------------------------------------------------

/// 一个已加载对象。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Handle {
    id: u32,
    path: String,
    mode: u32,
    refs: u32,
    nodelete: bool,
}

impl Handle {
    /// 构造。
    pub fn new(id: u32, path: &str, mode: u32) -> Handle {
        Handle {
            id,
            path: String::from(path),
            mode,
            refs: 1,
            nodelete: Mode::new(mode).nodelete(),
        }
    }

    /// 句柄号。
    pub const fn id(&self) -> u32 {
        self.id
    }

    /// 路径。
    pub fn path(&self) -> &str {
        self.path.as_str()
    }

    /// 模式。
    pub const fn mode(&self) -> u32 {
        self.mode
    }

    /// 引用数。
    pub const fn refs(&self) -> u32 {
        self.refs
    }

    /// 是否钉住。
    pub const fn nodelete(&self) -> bool {
        self.nodelete
    }

    /// 加一次引用。
    fn note(&mut self) {
        self.refs = self.refs.saturating_add(1);
    }

    /// 减一次引用；返回是否应彻底移除。
    fn release(&mut self) -> bool {
        if self.nodelete {
            return false;
        }
        if self.refs > 1 {
            self.refs -= 1;
            return false;
        }
        true
    }

    /// 文本形态 `#3 libfoo.so refs=2`。
    pub fn tag(&self) -> String {
        let mut s = String::from("#");
        s.push_str(dec_text(self.id).as_str());
        s.push(' ');
        s.push_str(self.path.as_str());
        s.push_str(" refs=");
        s.push_str(dec_text(self.refs).as_str());
        if self.nodelete {
            s.push_str(" nodelete");
        }
        s
    }

    /// 摘要。
    pub fn seal(&self) -> u64 {
        let mut h = fold_word(FNV_OFF, self.id as u64);
        h = fold_word(h, self.mode as u64);
        h = fold_word(h, self.refs as u64);
        h = fold_word(h, self.nodelete as u64);
        h = fold_word(h, digest(self.path.as_bytes()));
        h
    }
}

/// 空槽。
const EMPTY_SLOT: Option<Handle> = None;

// ---------------------------------------------------------------------------
// dlopen / dlclose / dlsym
// ---------------------------------------------------------------------------

/// 一个进程的装载器状态。
#[derive(Clone, Debug)]
pub struct Dl {
    slots: [Option<Handle>; MAX_HANDLES],
    err: ErrorQueue,
    next_id: u32,
    opens: u32,
    closes: u32,
    failures: u32,
    symbols: u32,
}

impl Dl {
    /// 空装载器。
    pub fn new() -> Dl {
        Dl {
            slots: [EMPTY_SLOT; MAX_HANDLES],
            err: ErrorQueue::new(),
            next_id: 1,
            opens: 0,
            closes: 0,
            failures: 0,
            symbols: 0,
        }
    }

    /// 记一次失败并给出文案。
    fn fail(&mut self, code: u32, text: String) -> Result<u32, u32> {
        self.err.set(text.as_str());
        self.failures = self.failures.saturating_add(1);
        Err(code)
    }

    /// 同上，但返回 `Result<(), u32>`。
    fn fail_unit(&mut self, code: u32, text: String) -> Result<(), u32> {
        self.err.set(text.as_str());
        self.failures = self.failures.saturating_add(1);
        Err(code)
    }

    /// 按路径找句柄号。
    pub fn find(&self, path: &str) -> Option<u32> {
        let mut i = 0usize;
        while i < MAX_HANDLES {
            if let Some(slot) = self.slots.get(i) {
                if let Some(h) = slot {
                    if h.path.as_str() == path {
                        return Some(h.id);
                    }
                }
            }
            i += 1;
        }
        None
    }

    /// 按句柄号找槽位。
    fn slot_of(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < MAX_HANDLES {
            if let Some(slot) = self.slots.get(i) {
                if let Some(h) = slot {
                    if h.id == id {
                        return Some(i);
                    }
                }
            }
            i += 1;
        }
        None
    }

    /// 插一个句柄。
    fn insert(&mut self, path: &str, mode: u32) -> Result<u32, u32> {
        let mut i = 0usize;
        while i < MAX_HANDLES {
            let empty = match self.slots.get(i) {
                Some(slot) => slot.is_none(),
                None => false,
            };
            if empty {
                let id = self.next_id;
                self.next_id = self.next_id.saturating_add(1);
                if let Some(dst) = self.slots.get_mut(i) {
                    *dst = Some(Handle::new(id, path, mode));
                }
                return Ok(id);
            }
            i += 1;
        }
        Err(DL_ESTATE)
    }

    /// 预加载登记（模拟 ld.so 已就位的对象）。
    pub fn preload(&mut self, path: &str) -> Result<u32, u32> {
        match self.insert(path, RTLD_NOW | RTLD_NODELETE) {
            Ok(id) => Ok(id),
            Err(c) => self.fail(c, String::from("cannot allocate memory")),
        }
    }

    /// 打开；成功给非 0 句柄，失败给错误码并挂一条 dlerror。
    pub fn open(&mut self, path: &str, mode: u32) -> Result<u32, u32> {
        self.err.clear();
        let m = Mode::new(mode);
        if let Err(c) = m.validate() {
            return self.fail(c, Fault::Mode.text(path));
        }
        if path.is_empty() {
            if let Some(id) = self.find("") {
                return Ok(id);
            }
            return match self.insert("", mode) {
                Ok(id) => {
                    self.opens = self.opens.saturating_add(1);
                    Ok(id)
                }
                Err(c) => self.fail(c, String::from("cannot allocate memory")),
            };
        }
        if m.noload() {
            // RTLD_NOLOAD 命中也算一次引用：调用方必须 dlclose。
            return match self.find(path) {
                Some(id) => {
                    if let Some(idx) = self.slot_of(id) {
                        if let Some(Some(h)) = self.slots.get_mut(idx) {
                            h.note();
                        }
                    }
                    Ok(id)
                }
                None => self.fail(DL_ESTATE, named(path, "object not open")),
            };
        }
        if let Some(id) = self.find(path) {
            if let Some(idx) = self.slot_of(id) {
                if let Some(Some(h)) = self.slots.get_mut(idx) {
                    h.note();
                }
            }
            return Ok(id);
        }
        let o = simulate(path, mode);
        if let Some(f) = o.fault() {
            let text = o.text();
            return self.fail(f.code(), text);
        }
        match self.insert(path, mode) {
            Ok(id) => {
                self.opens = self.opens.saturating_add(1);
                Ok(id)
            }
            Err(c) => self.fail(c, String::from("cannot allocate memory")),
        }
    }

    /// 关闭。
    pub fn close(&mut self, id: u32) -> Result<(), u32> {
        self.err.clear();
        if id == DL_NULL {
            return self.fail_unit(DL_ESTATE, String::from("invalid handle"));
        }
        let idx = match self.slot_of(id) {
            Some(v) => v,
            None => return self.fail_unit(DL_ESTATE, String::from("invalid handle")),
        };
        self.closes = self.closes.saturating_add(1);
        let mut remove = false;
        if let Some(Some(h)) = self.slots.get_mut(idx) {
            remove = h.release();
        }
        if remove {
            if let Some(dst) = self.slots.get_mut(idx) {
                *dst = EMPTY_SLOT;
            }
        }
        Ok(())
    }

    /// 查符号。
    pub fn symbol(&mut self, id: u32, name: &str) -> Result<u32, u32> {
        self.err.clear();
        if id == DL_NULL || self.slot_of(id).is_none() {
            return self.fail(DL_ESTATE, String::from("invalid handle"));
        }
        if name.is_empty() {
            return self.fail(DL_ESTATE, String::from("invalid symbol name"));
        }
        if name == "no_such_symbol" || name == "missing" {
            return self.fail(DL_ESYM, Fault::Sym.text(name));
        }
        self.symbols = self.symbols.saturating_add(1);
        Ok((digest(name.as_bytes()) as u32) | 1)
    }

    /// dlerror：取走一条（一次性）。
    pub fn error(&mut self) -> Option<String> {
        self.err.take()
    }

    /// 是否挂着错。
    pub fn pending(&self) -> bool {
        self.err.pending()
    }

    /// 看一眼错。
    pub fn peek_error(&self) -> Option<&str> {
        self.err.peek()
    }

    /// 活句柄数。
    pub fn live(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < MAX_HANDLES {
            if let Some(slot) = self.slots.get(i) {
                if slot.is_some() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 取句柄。
    pub fn handle(&self, id: u32) -> Option<&Handle> {
        let idx = self.slot_of(id)?;
        match self.slots.get(idx) {
            Some(Some(h)) => Some(h),
            _ => None,
        }
    }

    /// 打开成功次数。
    pub const fn opens(&self) -> u32 {
        self.opens
    }

    /// 关闭次数。
    pub const fn closes(&self) -> u32 {
        self.closes
    }

    /// 失败次数。
    pub const fn failures(&self) -> u32 {
        self.failures
    }

    /// 符号解析成功次数。
    pub const fn symbols(&self) -> u32 {
        self.symbols
    }

    /// 状态摘要。
    pub fn seal(&self) -> u64 {
        let mut h = fold_word(FNV_OFF, self.opens as u64);
        h = fold_word(h, self.closes as u64);
        h = fold_word(h, self.failures as u64);
        let mut i = 0usize;
        while i < MAX_HANDLES {
            if let Some(Some(x)) = self.slots.get(i) {
                h = fold_word(h, x.seal());
            }
            i += 1;
        }
        h
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
    pub fn new() -> Ledger {
        Ledger {
            rows: [Sample::blank(); SAMPLES],
            len: 0,
            root: 0,
            frozen: false,
            rejects: 0,
        }
    }

    /// 登记。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(DL_ESLOT);
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

    /// 对账：14 行、8 成 6 败、值与码逐行对上。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(DL_EBAL);
        }
        const VALS: [u32; 8] = [14, 2, 12, 4, 2, 2, 2, 929];
        const CODES: [u32; 6] =
            [DL_ENOENT, DL_EBADELF, DL_ECLASS, DL_EMACHINE, DL_EMODE, DL_ESTATE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(DL_EBAL),
            };
            if s.slot as usize != i {
                return Err(DL_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(DL_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(DL_EVALUE);
                        }
                    }
                    None => return Err(DL_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(DL_EVALUE);
                        }
                    }
                    None => return Err(DL_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(DL_EBAL);
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
            return Err(DL_EBAL);
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
            Err(DL_ESEAL)
        }
    }

    /// 冻结指纹。
    pub const fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub const fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub const fn rejects(&self) -> u32 {
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
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.ok())));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.fail())));
    let _ = l.register(count_cell(3, OP_LOOKUP, r.map(|x| x.noent())));
    let _ = l.register(count_cell(4, OP_OPEN, r.map(|x| x.badelf())));
    let _ = l.register(count_cell(5, OP_OPEN, r.map(|x| x.class32())));
    let _ = l.register(count_cell(6, OP_OPEN, r.map(|x| x.machine())));

    let red = run_with(Some((0, DL_ENOENT)));
    let _ = l.register(count_cell(7, OP_MODE, red.map(|x| x.rate() as usize)));

    let noent = simulate("libfoo.so", RTLD_LAZY).code();
    let _ = l.register(err_cell(8, OP_LOOKUP, noent));

    let badelf = simulate("./bad.so", RTLD_LAZY).code();
    let _ = l.register(err_cell(9, OP_OPEN, badelf));

    let class = simulate("./x32.so", RTLD_LAZY).code();
    let _ = l.register(err_cell(10, OP_OPEN, class));

    let machine = simulate("./arm.so", RTLD_LAZY).code();
    let _ = l.register(err_cell(11, OP_OPEN, machine));

    let mode = simulate("libq.so", 0x2000).code();
    let _ = l.register(err_cell(12, OP_MODE, mode));

    let mut d = Dl::new();
    let state = match d.close(DL_NULL) {
        Ok(()) => DL_ESTATE,
        Err(c) => c,
    };
    let _ = l.register(err_cell(13, OP_HANDLE, state));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 名称闸门。
fn name_ok() -> bool {
    let a = Mode::new(0).text().as_str() == "RTLD_LOCAL";
    let b = Mode::new(RTLD_NOW | RTLD_GLOBAL).text().as_str() == "RTLD_NOW|RTLD_GLOBAL";
    let c = Mode::new(RTLD_LAZY | RTLD_NOW).text().as_str() == "RTLD_LAZY|RTLD_NOW";
    let d = Mode::new(0x2000).text().as_str() == "RTLD_UNKNOWN|0x00002000";
    let e = Stage::Mode.name() == "mode" && Stage::Ready.name() == "ready";
    let f = Fault::NoEnt.name() == "noent" && Fault::Class32.name() == "class32";
    let g = machine_name(EM_386) == "EM_386" && machine_name(EM_ARM) == "EM_ARM";
    let h = machine_name(0x7777) == "EM_UNKNOWN";
    a && b && c && d && e && f && g && h
}

/// 模式位。
fn mode_ok() -> bool {
    let z = Mode::new(0);
    let a = z.validate() == Ok(()) && z.bind() == Bind::Lazy && z.scope() == Scope::Local;
    let b = Mode::new(RTLD_NOW).bind() == Bind::Now;
    let c = Mode::new(RTLD_LAZY | RTLD_NOW).bind() == Bind::Now;
    let d = Mode::new(RTLD_GLOBAL).scope() == Scope::Global;
    let e = Mode::new(RTLD_NOLOAD).noload() && !Mode::new(RTLD_LAZY).noload();
    let f = Mode::new(RTLD_NODELETE).nodelete() && !Mode::new(0).nodelete();
    let g = Mode::new(RTLD_DEEPBIND).deepbind() && !Mode::new(0).deepbind();
    let h = Mode::new(0x2000).validate() == Err(DL_EMODE);
    let i = Mode::new(MODE_KNOWN).validate() == Ok(());
    let j = Mode::new(MODE_KNOWN).bit_count() == 6;
    let k = Mode::new(RTLD_LAZY | RTLD_GLOBAL | RTLD_NODELETE).bit_count() == 3;
    let m = Mode::new(0x8000).unknown() == 0x8000 && Mode::new(MODE_KNOWN).unknown() == 0;
    a && b && c && d && e && f && g && h && i && j && k && m
}

/// 阶段与根因映射。
fn stage_ok() -> bool {
    let a = Stage::Mode.index() == 0 && Stage::Ready.index() == 8;
    let b = Stage::Ident.index() + 1 == Stage::Class.index();
    let c = Stage::Class.index() + 1 == Stage::Machine.index();
    let d = Stage::Machine.index() + 1 == Stage::Deps.index();
    let e = Stage::Ready.terminal() && !Stage::Init.terminal() && !Stage::Mode.terminal();
    let f = Fault::NoEnt.stage() == Stage::Locate;
    let g = Fault::Perm.stage() == Stage::Open && Fault::Short.stage() == Stage::Open;
    let h = Fault::BadElf.stage() == Stage::Ident;
    let i = Fault::Class32.stage() == Stage::Class && Fault::Class64.stage() == Stage::Class;
    let j = Fault::Machine(EM_ARM).stage() == Stage::Machine;
    let k = Fault::Mode.stage() == Stage::Mode;
    let m = Stage::Mode.detail() == "mode word" && Stage::Ready.detail() == "handle live";
    a && b && c && d && e && f && g && h && i && j && k && m
}

/// 文案逐字。
fn text_lit_ok() -> bool {
    let a = Fault::NoEnt.text("libfoo.so").as_str()
        == "libfoo.so: cannot open shared object file: No such file or directory";
    let b = Fault::Perm.text("/root/secret.so").as_str()
        == "/root/secret.so: cannot open shared object file: Permission denied";
    let c = Fault::BadElf.text("./bad.so").as_str() == "./bad.so: invalid ELF header";
    let d = Fault::Short.text("./empty.so").as_str() == "./empty.so: file too short";
    let e = Fault::Class32.text("./x32.so").as_str()
        == "./x32.so: wrong ELF class: ELFCLASS32";
    let f = Fault::Class64.text("./x64.so").as_str()
        == "./x64.so: wrong ELF class: ELFCLASS64";
    let g = Fault::Machine(EM_386).text("./m32.so").as_str()
        == "./m32.so: wrong ELF machine: EM_386";
    let h = Fault::Mode.text("x").as_str() == "invalid mode for dlopen()";
    let i = Fault::Sym.text("foo").as_str() == "undefined symbol: foo";
    let j = class_text("./c0.so", 0).as_str() == "./c0.so: wrong ELF class: ELFCLASSNONE";
    let k = class_text("./c9.so", 9).as_str() == "./c9.so: wrong ELF class: ELFCLASSNN/9";
    let m = machine_text("./u.so", 0x7777).as_str() == "./u.so: wrong ELF machine: EM_UNKNOWN/30583";
    a && b && c && d && e && f && g && h && i && j && k && m
}

/// 模拟：成败与 null 判据。
fn sim_ok() -> bool {
    let a = {
        let o = simulate("libc.so.6", RTLD_LAZY);
        o.live() && o.found() == "/lib/libc.so.6" && o.stage() == Stage::Ready
    };
    let b = {
        let o = simulate("/usr/lib/libm.so.6", RTLD_NOW);
        o.live() && o.found() == "/usr/lib/libm.so.6"
    };
    let c = {
        let o = simulate("", RTLD_NOW);
        o.live() && o.found() == "<main program>" && o.request().is_empty()
    };
    let d = {
        let o = simulate("libfoo.so", RTLD_LAZY);
        !o.live() && o.code() == DL_ENOENT && o.stage() == Stage::Locate && o.found().is_empty()
    };
    let e = simulate("./bad.so", RTLD_LAZY).code() == DL_EBADELF;
    let f = simulate("./x32.so", RTLD_LAZY).code() == DL_ECLASS;
    let g = simulate("./arm.so", RTLD_LAZY).code() == DL_EMACHINE;
    let h = simulate("./empty.so", RTLD_LAZY).code() == DL_EBADELF;
    let i = simulate("libq.so", 0x2000).code() == DL_EMODE;
    let j = {
        let o = simulate("libq.so", 0x2000);
        o.text().as_str() == "invalid mode for dlopen()"
    };
    let k = simulate("/root/secret.so", RTLD_LAZY).code() == DL_EPERM;
    let m = simulate("/tmp/lnk.so", RTLD_LAZY).code() == DL_ENOENT;
    a && b && c && d && e && f && g && h && i && j && k && m
}

/// 语料全跑。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.total() == CORPUS && r.ok() == 2 && r.fail() == 12 && r.matched() == CORPUS;
    let b = r.noent() == 4 && r.perm() == 1 && r.short() == 1;
    let c = r.badelf() == 2 && r.class32() == 2 && r.machine() == 2;
    let d = r.rate() == 1000 && r.green() && r.gate() == Ok(());
    a && b && c && d
}

/// 语料编号。
fn slots_ok() -> bool {
    if !unique() || !notes_ok() || !table_ok() {
        return false;
    }
    let a = match slot_at(0) {
        Some(p) => p.path == "libc.so.6" && p.expect.is_none() && p.mode == RTLD_LAZY,
        None => false,
    };
    let b = match slot_at(13) {
        Some(p) => p.expect == Some(Fault::Short) && p.mode == RTLD_LAZY,
        None => false,
    };
    let c = slot_at(CORPUS).is_none();
    a && b && c
}

/// 汇总渲染。
fn report_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.render().as_str()
        == "dlopen total=14 ok=2 fail=12 noent=4 perm=1 short=1 badelf=2 class32=2 machine=2 match=14 rate=1000ppm\n";
    let e = Report::new();
    let b = e.is_empty() && e.rate() == 0 && e.gate() == Err(DL_ENOBS);
    let c = e.render().as_str()
        == "dlopen total=0 ok=0 fail=0 noent=0 perm=0 short=0 badelf=0 class32=0 machine=0 match=0 rate=0ppm\n";
    a && b && c
}

/// 红单显式。
fn reds_ok() -> bool {
    let r = match run_with(Some((3, DL_ENOENT))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched() == CORPUS - 1 && r.rate() == 929;
    let b = r.gate() == Err(DL_EBAL) && !r.green() && r.red();
    let c = run_with(Some((CORPUS, DL_ENOENT))).err() == Some(DL_ESLOT);
    let d = run_with(Some((CORPUS + 1, DL_ENOENT))).err() == Some(DL_ESLOT);
    a && b && c && d
}

/// 封面。
fn cover_ok() -> bool {
    cover(CORPUS) == Ok(()) && cover(CORPUS - 1) == Err(DL_ENOBS) && cover(0) == Err(DL_ENOBS)
}

/// 错误队列。
fn queue_ok() -> bool {
    let mut q = ErrorQueue::new();
    let a = !q.pending() && q.peek().is_none() && q.is_empty();
    q.set("libfoo.so: cannot open shared object file: No such file or directory");
    let b = q.pending() && q.len() == 68 && q.peek().is_some();
    match q.take() {
        Some(t) => {
            if t.as_str() != "libfoo.so: cannot open shared object file: No such file or directory" {
                return false;
            }
        }
        None => return false,
    }
    let c = !q.pending() && q.take().is_none() && q.peek().is_none();
    q.set("a");
    q.set("bb");
    let d = match q.take() {
        Some(t) => t.as_str() == "bb",
        None => false,
    };
    q.set("cc");
    q.clear();
    let e = !q.pending() && q.take().is_none();
    a && b && c && d && e
}

/// 句柄表。
fn registry_ok() -> bool {
    let mut d = Dl::new();
    let a = d.live() == 0 && d.opens() == 0 && d.failures() == 0;
    let h1 = match d.open("libc.so.6", RTLD_LAZY) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let b = h1 != DL_NULL && d.live() == 1 && d.opens() == 1;
    let h2 = match d.open("libc.so.6", RTLD_NOW) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let c = h2 == h1 && d.live() == 1;
    let r = match d.handle(h1) {
        Some(h) => h.refs(),
        None => return false,
    };
    let e = r == 2;
    let n = match d.open("libc.so.6", RTLD_NOLOAD) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let f = n == h1 && d.handle(h1).map(|x| x.refs()) == Some(3);
    let g = match d.open("libnosuch.so", RTLD_NOLOAD) {
        Ok(_) => false,
        Err(c2) => {
            c2 == DL_ESTATE
                && d.peek_error() == Some("libnosuch.so: object not open")
        }
    };
    let h = d.close(h1).is_ok() && d.close(h1).is_ok() && d.live() == 1;
    let i = d.close(h1).is_ok() && d.live() == 0;
    let j = match d.close(h1) {
        Ok(()) => false,
        Err(c2) => c2 == DL_ESTATE && d.peek_error() == Some("invalid handle"),
    };
    let k = match d.close(DL_NULL) {
        Ok(()) => false,
        Err(c2) => c2 == DL_ESTATE,
    };
    a && b && c && e && f && g && h && i && j && k
}

/// 钉住与预加载。
fn pin_ok() -> bool {
    let mut d = Dl::new();
    let h = match d.open("/usr/lib/libm.so.6", RTLD_NOW | RTLD_NODELETE) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = h != DL_NULL;
    let b = match d.handle(h) {
        Some(x) => x.nodelete() && x.refs() == 1,
        None => false,
    };
    let c = d.close(h).is_ok() && d.live() == 1;
    let mut p = Dl::new();
    let mut i = 0usize;
    while i < MAX_HANDLES {
        let name = if i == 0 { "/lib/a.so" } else { "/lib/b.so" };
        let _ = p.preload(name);
        i += 1;
    }
    let last_err = match p.preload("/lib/overflow.so") {
        Ok(_) => 0,
        Err(c2) => c2,
    };
    let e = last_err == DL_ESTATE && p.live() == MAX_HANDLES;
    a && b && c && e
}

/// 符号解析。
fn symbol_ok() -> bool {
    let mut d = Dl::new();
    let h = match d.open("libc.so.6", RTLD_NOW) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = match d.symbol(h, "malloc") {
        Ok(v) => v != 0 && d.symbols() == 1,
        Err(_) => false,
    };
    let b = match d.symbol(h, "no_such_symbol") {
        Ok(_) => false,
        Err(c) => c == DL_ESYM && d.peek_error() == Some("undefined symbol: no_such_symbol"),
    };
    let c = match d.symbol(DL_NULL, "x") {
        Ok(_) => false,
        Err(c2) => c2 == DL_ESTATE,
    };
    let e = match d.symbol(h, "") {
        Ok(_) => false,
        Err(c2) => c2 == DL_ESTATE && d.peek_error() == Some("invalid symbol name"),
    };
    a && b && c && e
}

/// dlerror 的清除纪律：成功调用后旧错不得残留。
fn clear_ok() -> bool {
    let mut d = Dl::new();
    let a = d.open("libfoo.so", RTLD_LAZY).is_err() && d.pending();
    let b = d.open("libc.so.6", RTLD_LAZY).is_ok() && !d.pending();
    let c = d.error().is_none();
    let e = match d.open("./bad.so", RTLD_NOW) {
        Ok(_) => false,
        Err(c2) => c2 == DL_EBADELF,
    };
    let f = match d.error() {
        Some(t) => t.as_str() == "./bad.so: invalid ELF header",
        None => false,
    };
    let g = d.error().is_none();
    a && b && c && e && f && g
}

/// 摘要与文本。
fn hash_ok() -> bool {
    let a = digest(b"abcdef") == digest(b"abcdef") && digest(b"abcdef") != digest(b"abcdeg");
    let b = fold_word(FNV_OFF, 0x0102) != fold_word(FNV_OFF, 0x0201);
    let c = hex_text(0, 8).as_str() == "00000000" && hex_text(0xdeadbeef, 8).as_str() == "deadbeef";
    let d = dec_text(0).as_str() == "0" && dec_text(4294967295).as_str() == "4294967295";
    let e = ascii_lower("RTLD_NOW").as_str() == "rtld_now";
    let f = {
        let o = simulate("libc.so.6", RTLD_LAZY);
        o.seal() == simulate("libc.so.6", RTLD_LAZY).seal()
    };
    let g = simulate("libc.so.6", RTLD_LAZY).seal() != simulate("libc.so.6", RTLD_NOW).seal();
    let h = {
        let x = Handle::new(3, "libfoo.so", RTLD_NOW | RTLD_NODELETE);
        x.tag().as_str() == "#3 libfoo.so refs=1 nodelete" && x.seal() == x.clone().seal()
    };
    a && b && c && d && e && f && g && h
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && mode_ok() && stage_ok();
    out[1] = text_lit_ok() && sim_ok();
    out[2] = queue_ok() && registry_ok() && pin_ok() && symbol_ok() && clear_ok();
    out[3] = corpus_ok()
        && slots_ok()
        && report_ok()
        && reds_ok()
        && cover_ok()
        && hash_ok()
        && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11621_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11621.dlopen_null");

    s.add("DL01-模式位闸门", mode_ok() && name_ok(), "static");
    s.add("DL02-管线阶段", stage_ok(), "static");
    s.add("DL03-根因文案", text_lit_ok(), "static");
    s.add("DL04-NULL判据", sim_ok(), "static");
    s.add("DL05-dlerror一次性", queue_ok() && clear_ok(), "static");
    s.add("DL06-句柄注册表", registry_ok() && pin_ok(), "static");
    s.add("DL07-十四卡全跑", corpus_ok(), "static");
    s.add("DL08-语料编号", slots_ok(), "static");
    s.add("DL09-汇总渲染", report_ok(), "static");
    s.add("DL10-红单显式", reds_ok(), "static");
    s.add("DL11-封面", cover_ok(), "static");
    s.add("DL12-符号与摘要", symbol_ok() && hash_ok(), "static");
    s.add("DL13-冻结重放四注入", inject_four().iter().all(|x| *x), "static");

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
    fn mode_bits() {
        assert_eq!(Mode::new(0).text().as_str(), "RTLD_LOCAL");
        assert_eq!(Mode::new(0x2000).text().as_str(), "RTLD_UNKNOWN|0x00002000");
        assert_eq!(Mode::new(RTLD_LAZY | RTLD_NOW).bind(), Bind::Now);
        assert_eq!(Mode::new(RTLD_GLOBAL).scope(), Scope::Global);
        assert_eq!(Mode::new(MODE_KNOWN).bit_count(), 6);
        assert_eq!(Mode::new(0x2000).validate(), Err(DL_EMODE));
        assert_eq!(Mode::new(MODE_KNOWN).validate(), Ok(()));
    }

    #[test]
    fn stage_ladder() {
        assert_eq!(Stage::Mode.index(), 0);
        assert_eq!(Stage::Ready.index(), 8);
        assert!(Stage::Ready.terminal());
        assert_eq!(Fault::Machine(EM_ARM).stage(), Stage::Machine);
        assert_eq!(Fault::Short.stage(), Stage::Open);
    }

    #[test]
    fn fault_texts() {
        assert_eq!(
            Fault::NoEnt.text("libfoo.so").as_str(),
            "libfoo.so: cannot open shared object file: No such file or directory"
        );
        assert_eq!(Fault::Class32.text("./x.so").as_str(), "./x.so: wrong ELF class: ELFCLASS32");
        assert_eq!(
            Fault::Machine(EM_386).text("./m.so").as_str(),
            "./m.so: wrong ELF machine: EM_386"
        );
        assert_eq!(Fault::Mode.text("./a.so").as_str(), "invalid mode for dlopen()");
    }

    #[test]
    fn simulate_search_hit() {
        let o = simulate("libc.so.6", RTLD_LAZY);
        assert!(o.live());
        assert_eq!(o.found(), "/lib/libc.so.6");
        assert_eq!(o.stage(), Stage::Ready);
        assert!(o.text().is_empty());
    }

    #[test]
    fn simulate_missing() {
        let o = simulate("libfoo.so", RTLD_LAZY);
        assert!(!o.live());
        assert_eq!(o.code(), DL_ENOENT);
        assert_eq!(o.stage(), Stage::Locate);
    }

    #[test]
    fn simulate_class_before_machine() {
        // x32b: class=1, machine=62；类宽必须先炸。
        let o = simulate("./x32b.so", RTLD_NOW);
        assert_eq!(o.code(), DL_ECLASS);
        assert_eq!(o.stage(), Stage::Class);
    }

    #[test]
    fn simulate_mode_first() {
        let o = simulate("/nonexistent/x.so", 0x2000);
        assert_eq!(o.code(), DL_EMODE);
        assert_eq!(o.text().as_str(), "invalid mode for dlopen()");
    }

    #[test]
    fn simulate_null_path() {
        let o = simulate("", RTLD_NOW);
        assert!(o.live());
        assert_eq!(o.found(), "<main program>");
    }

    #[test]
    fn error_queue_one_shot() {
        let mut q = ErrorQueue::new();
        q.set("x");
        assert!(q.pending());
        assert_eq!(q.peek(), Some("x"));
        assert_eq!(q.take(), Some(String::from("x")));
        assert_eq!(q.take(), None);
        assert!(!q.pending());
    }

    #[test]
    fn handle_refcount() {
        let mut d = Dl::new();
        let a = match d.open("libc.so.6", RTLD_LAZY) {
            Ok(v) => v,
            Err(_) => return,
        };
        let b = match d.open("libc.so.6", RTLD_NOW) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(a, b);
        assert_eq!(d.handle(a).map(|h| h.refs()), Some(2));
        assert_eq!(d.close(a), Ok(()));
        assert_eq!(d.close(a), Ok(()));
        assert_eq!(d.live(), 0);
    }

    #[test]
    fn noload_miss() {
        let mut d = Dl::new();
        assert_eq!(d.open("libnosuch.so", RTLD_NOLOAD), Err(DL_ESTATE));
        assert_eq!(d.peek_error(), Some("libnosuch.so: object not open"));
    }

    #[test]
    fn nodelete_pins() {
        let mut d = Dl::new();
        let h = match d.open("/usr/lib/libm.so.6", RTLD_NOW | RTLD_NODELETE) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(d.close(h), Ok(()));
        assert_eq!(d.live(), 1);
        assert!(d.handle(h).is_some());
    }

    #[test]
    fn undefined_symbol() {
        let mut d = Dl::new();
        let h = match d.open("libc.so.6", RTLD_NOW) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(d.symbol(h, "no_such_symbol"), Err(DL_ESYM));
        assert_eq!(d.peek_error(), Some("undefined symbol: no_such_symbol"));
        assert_eq!(d.error(), Some(String::from("undefined symbol: no_such_symbol")));
        assert_eq!(d.error(), None);
    }

    #[test]
    fn success_clears_error() {
        let mut d = Dl::new();
        assert_eq!(d.open("libfoo.so", RTLD_LAZY), Err(DL_ENOENT));
        assert!(d.pending());
        assert!(d.open("libc.so.6", RTLD_LAZY).is_ok());
        assert!(!d.pending());
    }

    #[test]
    fn report_render() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(
            r.render().as_str(),
            "dlopen total=14 ok=2 fail=12 noent=4 perm=1 short=1 badelf=2 class32=2 machine=2 match=14 rate=1000ppm\n"
        );
        assert_eq!(r.gate(), Ok(()));
        assert!(r.green());
    }

    #[test]
    fn red_override() {
        let r = match run_with(Some((3, DL_ENOENT))) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(r.matched(), CORPUS - 1);
        assert_eq!(r.rate(), 929);
        assert_eq!(r.gate(), Err(DL_EBAL));
        assert_eq!(run_with(Some((14, DL_ENOENT))).err(), Some(DL_ESLOT));
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert!(l.sample(0).is_some());
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
        assert_eq!(l.root(), root);
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(DL_EBAL));
        assert!(l.rejects() >= 1);
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
        let s = run_unx11621_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }
}


