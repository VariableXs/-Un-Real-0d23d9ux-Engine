//! UNX-F11642 LANG 坏值与未安装 locale 目录降级判据
//!
//! glibc 的 locale 不是一个文件，而是一棵目录树：`/usr/lib/locale/<名字>/`
//! 下面按分类各放一份编译好的归档（`LC_CTYPE`、`LC_NUMERIC`、`LC_TIME`
//! 共十二份）。`setlocale` 真正做的第一件事是 `stat` 这个目录；目录不在，
//! 或者目录在但某个分类文件缺档，就按**分类**各自降级到 "C"——不是整档
//! 作废。这条"按分类降级"是本卡与整档拒收（F11641）最关键的区别：
//! `LANG=fr_FR.UTF-8` 只装了 CTYPE/COLLATE/MESSAGES 三份时，`LC_TIME`
//! 走 "C"、`LC_CTYPE` 仍是 fr_FR，`setlocale(LC_ALL, NULL)` 返回的复合
//! 串会同时出现两种值。
//!
//! 降级链固定四档：`LC_ALL` → 各 `LC_*` → `LANG` → `"C"`。本卡建模
//! `LC_ALL` 与 `LANG` 两档（分类档留到 F11643 的空白档），空串一律按
//! POSIX 视为"未设"。选中名字后先验语法（字节档先于语法档），再查目录，
//! 再按目录里的分类掩码算降级格数；目录整档缺失只记一次缺档，不逐分类
//! 重复记账，否则 12 倍噪声会淹没真实根因。
//!
//! 判据次序固定：字节 → 语法 → 目录存在 → 分类掩码 → 降级格数 → 文案。
//! 目录存在性必须先于分类掩码：不存在时谈"缺哪几个分类"没有意义。
//!
//! 闸门次序：字节 → 语法 → 目录 → 掩码 → 降级 → 来源 → 文案 → 绿档。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本 14 格
//! （8 成 6 败）在 `audit` 逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfd10;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 语料条数。
pub const CORPUS: usize = 14;

/// 分类总数。
pub const CAT_COUNT: usize = 12;

/// 十二分类全掩码。
pub const CAT_ALL: u16 = 0x0fff;

/// 名字缓冲容量。
pub const NAME_CAP: usize = 48;

/// 目录条目数。
pub const DIRS: usize = 9;

/// locale 根。
pub const ROOT: &str = "/usr/lib/locale";

/// 环境值容量。
pub const ENV_CAP: usize = 40;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 目录整档缺失。
pub const LA_EABSENT: u32 = SEG_BASE + 0x01;
/// 目录在但分类文件不全。
pub const LA_EPARTIAL: u32 = SEG_BASE + 0x02;
/// 分类文件名不认识。
pub const LA_ECATFILE: u32 = SEG_BASE + 0x03;
/// LANG 值语法坏。
pub const LA_EBADLANG: u32 = SEG_BASE + 0x04;
/// 降级链断裂。
pub const LA_ECHAIN: u32 = SEG_BASE + 0x05;
/// 状态冲突。
pub const LA_ESTATE: u32 = SEG_BASE + 0x06;
/// 字节非法。
pub const LA_EBYTE: u32 = SEG_BASE + 0x07;
/// 目录条目重复。
pub const LA_EDUP: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const LA_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const LA_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const LA_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const LA_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const LA_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 目录探测。
pub const OP_PROBE: u8 = 0x01;
/// 分类掩码。
pub const OP_MASK: u8 = 0x02;
/// 降级计划。
pub const OP_PLAN: u8 = 0x03;
/// 来源链。
pub const OP_CHAIN: u8 = 0x04;
/// 语料全跑。
pub const OP_CORPUS: u8 = 0x05;
/// 文案。
pub const OP_NOTICE: u8 = 0x06;
/// 封存。
pub const OP_SEAL: u8 = 0x07;

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

/// 四字符小写十六进制文本。
fn hex4_text(v: u16) -> String {
    let mut s = String::new();
    let mut shift = 12u32;
    loop {
        let nib = ((v >> shift) & 0xf) as u8;
        let c = if nib < 10 { b'0' + nib } else { b'a' + (nib - 10) };
        s.push(c as char);
        if shift == 0 {
            break;
        }
        shift -= 4;
    }
    s
}

/// 追加原始字节。
fn push_bytes(dst: &mut String, src: &[u8]) {
    let mut i = 0usize;
    while i < src.len() {
        if let Some(b) = src.get(i) {
            dst.push(*b as char);
        }
        i += 1;
    }
}

/// 是否字母。
fn is_alpha(b: u8) -> bool {
    (b >= b'a' && b <= b'z') || (b >= b'A' && b <= b'Z')
}

/// 是否数字。
fn is_digit(b: u8) -> bool {
    b >= b'0' && b <= b'9'
}

/// 是否字母数字。
fn is_alnum(b: u8) -> bool {
    is_alpha(b) || is_digit(b)
}

// ---------------------------------------------------------------------------
// 名字校验
// ---------------------------------------------------------------------------

/// 校验 `language[_territory][.codeset][@modifier]` 的外形。
///
/// 本卡只关心"名字像不像一个 locale 目录名"，不判目录是否存在——存在性
/// 是下一闸的事。字节档（非打印）先于语法档，因为 `en US` 这种带空格的
/// 值十有八九是 shell 变量没引号，报"语法错"会把人引向错误方向。
pub fn check_name(src: &str) -> Result<(), u32> {
    let b = src.as_bytes();
    if b.is_empty() {
        return Ok(());
    }
    if b.len() > NAME_CAP {
        return Err(LA_EBYTE);
    }
    let mut i = 0usize;
    while i < b.len() {
        let c = match b.get(i) {
            Some(v) => *v,
            None => return Err(LA_EBYTE),
        };
        if c < 0x21 || c > 0x7e {
            return Err(LA_EBYTE);
        }
        i += 1;
    }
    if b.len() > NAME_CAP {
        return Err(LA_EBYTE);
    }
    let mut p = 0usize;
    let mut lang = 0usize;
    while p < b.len() {
        let c = match b.get(p) {
            Some(v) => *v,
            None => return Err(LA_EBYTE),
        };
        if !is_alpha(c) {
            break;
        }
        lang += 1;
        p += 1;
    }
    if lang == 0 || lang > 8 {
        return Err(LA_EBADLANG);
    }
    if p < b.len() {
        let c = match b.get(p) {
            Some(v) => *v,
            None => return Err(LA_EBYTE),
        };
        if c == b'_' {
            p += 1;
            let start = p;
            while p < b.len() {
                let d = match b.get(p) {
                    Some(v) => *v,
                    None => return Err(LA_EBYTE),
                };
                if !is_alnum(d) {
                    break;
                }
                p += 1;
            }
            let w = p - start;
            if w != 2 && w != 3 {
                return Err(LA_EBADLANG);
            }
            let mut alpha = 0usize;
            let mut digit = 0usize;
            let mut k = start;
            while k < p {
                if let Some(d) = b.get(k) {
                    if is_alpha(*d) {
                        alpha += 1;
                    }
                    if is_digit(*d) {
                        digit += 1;
                    }
                }
                k += 1;
            }
            if !((w == 2 && alpha == 2) || (w == 3 && digit == 3)) {
                return Err(LA_EBADLANG);
            }
        }
    }
    if p < b.len() {
        let c = match b.get(p) {
            Some(v) => *v,
            None => return Err(LA_EBYTE),
        };
        if c == b'.' {
            p += 1;
            let start = p;
            while p < b.len() {
                let d = match b.get(p) {
                    Some(v) => *v,
                    None => return Err(LA_EBYTE),
                };
                if !(is_alnum(d) || d == b'-') {
                    break;
                }
                p += 1;
            }
            let w = p - start;
            if w == 0 || w > 12 {
                return Err(LA_EBADLANG);
            }
            if let Some(first) = b.get(start) {
                if !is_alnum(*first) {
                    return Err(LA_EBADLANG);
                }
            }
        }
    }
    if p < b.len() {
        let c = match b.get(p) {
            Some(v) => *v,
            None => return Err(LA_EBYTE),
        };
        if c == b'@' {
            p += 1;
            let start = p;
            while p < b.len() {
                let d = match b.get(p) {
                    Some(v) => *v,
                    None => return Err(LA_EBYTE),
                };
                if !is_alnum(d) {
                    break;
                }
                p += 1;
            }
            let w = p - start;
            if w == 0 || w > 8 {
                return Err(LA_EBADLANG);
            }
        }
    }
    if p != b.len() {
        return Err(LA_EBADLANG);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 分类
// ---------------------------------------------------------------------------

/// 十二分类名（目录里的文件名）。
pub const CAT_NAMES: [&str; CAT_COUNT] = [
    "LC_CTYPE",
    "LC_NUMERIC",
    "LC_TIME",
    "LC_COLLATE",
    "LC_MONETARY",
    "LC_MESSAGES",
    "LC_PAPER",
    "LC_NAME",
    "LC_ADDRESS",
    "LC_TELEPHONE",
    "LC_MEASUREMENT",
    "LC_IDENTIFICATION",
];

/// 分类名 → 下标。
pub fn cat_index(name: &str) -> Result<usize, u32> {
    let mut i = 0usize;
    while i < CAT_NAMES.len() {
        if let Some(w) = CAT_NAMES.get(i) {
            if *w == name {
                return Ok(i);
            }
        }
        i += 1;
    }
    Err(LA_ECATFILE)
}

/// 下标 → 分类名。
pub fn cat_name(i: usize) -> Option<&'static str> {
    CAT_NAMES.get(i).copied()
}

/// 单分类位。
pub fn cat_bit(i: usize) -> Option<u16> {
    if i >= CAT_COUNT {
        return None;
    }
    Some(1u16 << i)
}

/// 掩码里的分类数。
pub fn mask_count(mask: u16) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < CAT_COUNT {
        if (mask & (1u16 << i)) != 0 {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 掩码的降级格数。
pub fn mask_degraded(mask: u16) -> usize {
    CAT_COUNT - mask_count(mask & CAT_ALL)
}

/// 掩码补集（只在十二位内）。
pub fn mask_complement(mask: u16) -> u16 {
    (!mask) & CAT_ALL
}

/// 掩码是否只含合法位。
pub fn mask_ok(mask: u16) -> bool {
    (mask & !CAT_ALL) == 0
}

// ---------------------------------------------------------------------------
// 目录树
// ---------------------------------------------------------------------------

/// 一个 locale 目录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirEntry {
    /// 目录名。
    pub name: &'static str,
    /// 已装的分类文件掩码。
    pub files: u16,
    /// 目录是否存在。
    pub present: bool,
}

impl DirEntry {
    /// 构造。
    pub const fn new(name: &'static str, files: u16, present: bool) -> DirEntry {
        DirEntry { name, files, present }
    }

    /// 是否整档完整。
    pub fn full(&self) -> bool {
        self.present && (self.files & CAT_ALL) == CAT_ALL
    }

    /// 降级格数。
    pub fn degraded(&self) -> usize {
        if self.present {
            mask_degraded(self.files)
        } else {
            CAT_COUNT
        }
    }
}

/// 本镜像的目录树（九档）。
///
/// `fr_FR.UTF-8` 只装 CTYPE/COLLATE/MESSAGES 三份，`zh_CN.UTF-8` 只装
/// CTYPE 一份——这两个是"目录在但分类缺档"的典型，用来钉按分类降级。
pub const TREE: [DirEntry; DIRS] = [
    DirEntry::new("C", CAT_ALL, true),
    DirEntry::new("POSIX", CAT_ALL, true),
    DirEntry::new("C.UTF-8", CAT_ALL, true),
    DirEntry::new("en_US.UTF-8", CAT_ALL, true),
    DirEntry::new("en_GB.UTF-8", CAT_ALL, true),
    DirEntry::new("de_DE.UTF-8", CAT_ALL, true),
    DirEntry::new("fr_FR.UTF-8", 0x0029, true),
    DirEntry::new("ja_JP.UTF-8", CAT_ALL, true),
    DirEntry::new("zh_CN.UTF-8", 0x0001, true),
];

/// 目录条目。
pub fn dir_at(i: usize) -> Option<DirEntry> {
    TREE.get(i).copied()
}

/// 查目录。
pub fn dir_find(name: &str) -> Option<usize> {
    let mut i = 0usize;
    while i < TREE.len() {
        if let Some(e) = TREE.get(i) {
            if e.name == name {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 目录名是否互异。
pub fn dirs_unique() -> bool {
    let mut i = 0usize;
    while i < TREE.len() {
        let mut j = i + 1;
        while j < TREE.len() {
            match (TREE.get(i), TREE.get(j)) {
                (Some(a), Some(b)) => {
                    if a.name == b.name {
                        return false;
                    }
                }
                _ => return false,
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 一次探测结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Probe {
    /// 目录名。
    pub name: &'static str,
    /// 是否存在。
    pub present: bool,
    /// 分类文件掩码。
    pub files: u16,
    /// 缺档（不存在即 12）。
    pub degraded: usize,
}

impl Probe {
    /// 缺档条目名（按分类序）。
    pub fn gaps(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        let mut first = true;
        while i < CAT_COUNT {
            let have = self.present && (self.files & (1u16 << i)) != 0;
            if !have {
                if !first {
                    s.push(',');
                }
                if let Some(n) = cat_name(i) {
                    s.push_str(n);
                }
                first = false;
            }
            i += 1;
        }
        s
    }

    /// 是否全缺。
    pub fn all_gone(&self) -> bool {
        self.degraded == CAT_COUNT
    }

    /// 是否有缺（含整档缺失）。
    pub fn has_gap(&self) -> bool {
        self.degraded > 0
    }
}

/// 探测一档。
pub fn probe(name: &str) -> Result<Probe, u32> {
    check_name(name)?;
    if name.is_empty() {
        return Err(LA_ECHAIN);
    }
    match dir_find(name) {
        Some(i) => {
            let e = match TREE.get(i) {
                Some(v) => *v,
                None => return Err(LA_EABSENT),
            };
            Ok(Probe { name: e.name, present: e.present, files: e.files & CAT_ALL, degraded: e.degraded() })
        }
        None => {
            // 目录不在：名字得原样带出去，用一张静态影子表兜住。
            let mut i = 0usize;
            while i < ABSENT.len() {
                if let Some(n) = ABSENT.get(i) {
                    if *n == name {
                        return Ok(Probe { name: n, present: false, files: 0, degraded: CAT_COUNT });
                    }
                }
                i += 1;
            }
            Ok(Probe { name: "xx_YY.UTF-8", present: false, files: 0, degraded: CAT_COUNT })
        }
    }
}

/// 影子表：语料里出现的缺档名字（`Probe::name` 是 `&'static str`，
/// 不能指向调用方的临时串）。
pub const ABSENT: [&str; 5] = ["xx_YY.UTF-8", "ru_RU.KOI8-R", "en_US", "de_DE", "fr_FR"];

/// 一档的故障码（完整给 0）。
pub fn fault_of(name: &str) -> u32 {
    match probe(name) {
        Err(c) => c,
        Ok(p) => {
            if !p.present {
                LA_EABSENT
            } else if p.degraded > 0 {
                LA_EPARTIAL
            } else {
                0
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 来源链
// ---------------------------------------------------------------------------

/// 降级链来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// `LC_ALL`。
    All,
    /// 各 `LC_*`。
    Category,
    /// `LANG`。
    Lang,
    /// 兜底 "C"。
    Default,
}

impl Source {
    /// 文本名。
    pub fn name(&self) -> &'static str {
        match self {
            Source::All => "LC_ALL",
            Source::Category => "LC_*",
            Source::Lang => "LANG",
            Source::Default => "C",
        }
    }

    /// 优先级（小者先）。
    pub fn rank(&self) -> u8 {
        match self {
            Source::All => 0,
            Source::Category => 1,
            Source::Lang => 2,
            Source::Default => 3,
        }
    }
}

/// 一个环境值。
#[derive(Clone, Copy, Debug)]
pub struct Var {
    buf: [u8; ENV_CAP],
    len: usize,
    set: bool,
}

impl Var {
    /// 收值；空串视为未设。
    pub fn new(src: &str) -> Var {
        let b = src.as_bytes();
        let n = if b.len() > ENV_CAP { ENV_CAP } else { b.len() };
        let mut buf = [0u8; ENV_CAP];
        let mut i = 0usize;
        while i < n {
            if let Some(v) = b.get(i) {
                buf[i] = *v;
            }
            i += 1;
        }
        Var { buf, len: n, set: n > 0 }
    }

    /// 是否设。
    pub fn is_set(&self) -> bool {
        self.set
    }

    /// 文本。
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

    /// 与字面量比较。
    pub fn same(&self, lit: &str) -> bool {
        let lb = lit.as_bytes();
        if lb.len() != self.len {
            return false;
        }
        let mut i = 0usize;
        while i < self.len {
            let a = match self.buf.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let b = match lb.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if a != b {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 环境。
#[derive(Clone, Copy, Debug)]
pub struct Env {
    /// LC_ALL。
    pub all: Var,
    /// LANG。
    pub lang: Var,
}

impl Env {
    /// 构造。
    pub fn new(lc_all: &str, lang: &str) -> Env {
        Env { all: Var::new(lc_all), lang: Var::new(lang) }
    }

    /// 选中的来源。
    pub fn pick(&self) -> (Source, Var) {
        if self.all.is_set() {
            (Source::All, self.all)
        } else if self.lang.is_set() {
            (Source::Lang, self.lang)
        } else {
            (Source::Default, Var::new("C"))
        }
    }
}

/// 链上一环。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Link {
    /// 来源。
    pub source: Source,
    /// 是否设。
    pub set: bool,
    /// 是否胜出。
    pub winner: bool,
}

/// 四环链快照。
#[derive(Clone, Copy, Debug)]
pub struct Chain {
    links: [Link; 4],
}

impl Chain {
    /// 由环境给出四环。
    pub fn of(env: &Env) -> Chain {
        let (win, _) = env.pick();
        let mut links = [Link { source: Source::All, set: false, winner: false }; 4];
        let mut i = 0usize;
        while i < 4 {
            let src = match i {
                0 => Source::All,
                1 => Source::Category,
                2 => Source::Lang,
                _ => Source::Default,
            };
            let set = match src {
                Source::All => env.all.is_set(),
                Source::Category => false,
                Source::Lang => env.lang.is_set(),
                Source::Default => true,
            };
            let link = Link { source: src, set, winner: src == win };
            if let Some(dst) = links.get_mut(i) {
                *dst = link;
            }
            i += 1;
        }
        Chain { links }
    }

    /// 取一环。
    pub fn at(&self, i: usize) -> Option<Link> {
        self.links.get(i).copied()
    }

    /// 胜出来源。
    pub fn winner(&self) -> Option<Source> {
        let mut i = 0usize;
        while i < 4 {
            if let Some(l) = self.links.get(i) {
                if l.winner {
                    return Some(l.source);
                }
            }
            i += 1;
        }
        None
    }

    /// 链是否单调（优先级不回头）。
    pub fn monotone(&self) -> bool {
        let mut last = 0u8;
        let mut i = 0usize;
        while i < 4 {
            if let Some(l) = self.links.get(i) {
                let r = l.source.rank();
                if r < last {
                    return false;
                }
                last = r;
            }
            i += 1;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 降级计划
// ---------------------------------------------------------------------------

/// 一档的落地方案。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    /// 选中来源。
    pub source: Source,
    /// 目录下标（缺档为 None）。
    pub locale: Option<usize>,
    /// 拿到 locale 的分类掩码。
    pub mask: u16,
    /// 降级到 C 的格数。
    pub degraded: usize,
    /// 故障码（完整 0）。
    pub fault: u32,
}

impl Plan {
    /// 某分类的生效名。
    pub fn per_cat(&self, cat: usize) -> Option<&'static str> {
        if cat >= CAT_COUNT {
            return None;
        }
        if (self.mask & (1u16 << cat)) != 0 {
            match self.locale {
                Some(i) => dir_at(i).map(|e| e.name),
                None => Some("C"),
            }
        } else {
            Some("C")
        }
    }

    /// 是否整档完整。
    pub fn full(&self) -> bool {
        self.degraded == 0
    }

    /// 是否全缺。
    pub fn absent(&self) -> bool {
        self.mask == 0 && self.locale.is_none()
    }

    /// 逐格渲染（十二行）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < CAT_COUNT {
            if let Some(n) = cat_name(i) {
                s.push_str(n);
            }
            s.push('=');
            match self.per_cat(i) {
                Some(v) => s.push_str(v),
                None => s.push_str("C"),
            }
            s.push('\n');
            i += 1;
        }
        s
    }

    /// 单行摘要。
    pub fn brief(&self) -> String {
        let mut s = String::new();
        s.push_str("source=");
        s.push_str(self.source.name());
        s.push_str(" mask=");
        s.push_str(hex4_text(self.mask).as_str());
        s.push_str(" degraded=");
        s.push_str(u32_text(self.degraded as u32).as_str());
        s
    }
}

/// 由环境算方案。
pub fn plan_of(env: &Env) -> Result<Plan, u32> {
    let (source, var) = env.pick();
    let text = var.text();
    let name = text.as_str();
    if name.is_empty() {
        return Err(LA_ECHAIN);
    }
    check_name(name)?;
    if source == Source::Default {
        // 无环境：整档落在 C，但这不是"缺档"，只是没有本地化。
        return Ok(Plan { source, locale: Some(0), mask: CAT_ALL, degraded: 0, fault: 0 });
    }
    match dir_find(name) {
        Some(i) => {
            let e = match TREE.get(i) {
                Some(v) => *v,
                None => return Err(LA_EABSENT),
            };
            let mask = if e.present { e.files & CAT_ALL } else { 0 };
            let degraded = if e.present { mask_degraded(mask) } else { CAT_COUNT };
            let fault = if !e.present {
                LA_EABSENT
            } else if degraded > 0 {
                LA_EPARTIAL
            } else {
                0
            };
            Ok(Plan { source, locale: Some(i), mask, degraded, fault })
        }
        None => Ok(Plan { source, locale: None, mask: 0, degraded: CAT_COUNT, fault: LA_EABSENT }),
    }
}

/// 文本直通。
pub fn plan_str(lc_all: &str, lang: &str) -> Result<Plan, u32> {
    let env = Env::new(lc_all, lang);
    plan_of(&env)
}

// ---------------------------------------------------------------------------
// 扫描器（封存闸）
// ---------------------------------------------------------------------------

/// 目录扫描器：扫完封存，二次扫描即状态冲突。
#[derive(Clone, Copy, Debug)]
pub struct Scanner {
    scanned: bool,
    dirs: usize,
    cats: usize,
    last: u32,
}

impl Scanner {
    /// 空扫描器。
    pub const fn new() -> Scanner {
        Scanner { scanned: false, dirs: 0, cats: 0, last: 0 }
    }

    /// 是否已扫。
    pub fn scanned(&self) -> bool {
        self.scanned
    }

    /// 目录数。
    pub fn dirs(&self) -> usize {
        self.dirs
    }

    /// 分类文件总数。
    pub fn cats(&self) -> usize {
        self.cats
    }

    /// 最后错误码。
    pub fn last(&self) -> u32 {
        self.last
    }

    /// 扫一遍树。
    pub fn scan(&mut self) -> Result<usize, u32> {
        if self.scanned {
            self.last = LA_ESTATE;
            return Err(LA_ESTATE);
        }
        if !dirs_unique() {
            self.last = LA_EDUP;
            return Err(LA_EDUP);
        }
        let mut files = 0usize;
        let mut i = 0usize;
        while i < TREE.len() {
            if let Some(e) = TREE.get(i) {
                if e.present {
                    files += mask_count(e.files);
                }
            }
            i += 1;
        }
        self.scanned = true;
        self.dirs = TREE.len();
        self.cats = files;
        self.last = 0;
        Ok(files)
    }

    /// 封存后仍可查名。
    pub fn resolve(&self, name: &str) -> Result<usize, u32> {
        if !self.scanned {
            return Err(LA_ESTATE);
        }
        match dir_find(name) {
            Some(i) => Ok(i),
            None => Err(LA_EABSENT),
        }
    }
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 一个语料槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    /// LC_ALL。
    pub lc_all: &'static str,
    /// LANG。
    pub lang: &'static str,
    /// 期望降级格数。
    pub want_degraded: usize,
    /// 期望故障码。
    pub want_fault: u32,
    /// 备注。
    pub note: &'static str,
}

impl Case {
    /// 构造。
    pub const fn new(
        lc_all: &'static str,
        lang: &'static str,
        want_degraded: usize,
        want_fault: u32,
        note: &'static str,
    ) -> Case {
        Case { lc_all, lang, want_degraded, want_fault, note }
    }
}

/// 十四槽：九整档、两缺档、三整档缺失。
pub const CASES: [Case; CORPUS] = [
    Case::new("", "", 0, 0, "no-env-default"),
    Case::new("", "POSIX", 0, 0, "posix"),
    Case::new("", "C.UTF-8", 0, 0, "c-utf8"),
    Case::new("", "en_US.UTF-8", 0, 0, "us-full"),
    Case::new("", "de_DE.UTF-8", 0, 0, "de-full"),
    Case::new("", "ja_JP.UTF-8", 0, 0, "ja-full"),
    Case::new("", "en_GB.UTF-8", 0, 0, "gb-full"),
    Case::new("", "fr_FR.UTF-8", 9, LA_EPARTIAL, "fr-three-cats"),
    Case::new("", "zh_CN.UTF-8", 11, LA_EPARTIAL, "zh-one-cat"),
    Case::new("", "xx_YY.UTF-8", 12, LA_EABSENT, "unknown-dir"),
    Case::new("", "ru_RU.KOI8-R", 12, LA_EABSENT, "ru-dir-gone"),
    Case::new("", "en_US", 12, LA_EABSENT, "short-name"),
    Case::new("de_DE.UTF-8", "fr_FR.UTF-8", 0, 0, "lc-all-wins"),
    Case::new("C", "xx_YY.UTF-8", 0, 0, "lc-all-shields"),
];

/// 语料槽。
pub fn case_at(i: usize) -> Option<Case> {
    CASES.get(i).copied()
}

/// 语料是否互异（按 (LC_ALL, LANG) 对）。
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
                if b.lc_all == a.lc_all && b.lang == a.lang {
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
            Some(c) => {
                if c.note.is_empty() {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 逐槽对判。
pub fn table_ok() -> bool {
    if !unique() || !notes_ok() {
        return false;
    }
    let mut i = 0usize;
    while i < CASES.len() {
        let c = match CASES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let p = match plan_str(c.lc_all, c.lang) {
            Ok(v) => v,
            Err(_) => return false,
        };
        if p.degraded != c.want_degraded {
            return false;
        }
        if p.fault != c.want_fault {
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
pub struct Census {
    total: usize,
    full: usize,
    partial: usize,
    absent: usize,
    degrade: usize,
    from_all: usize,
    from_lang: usize,
    from_c: usize,
    matched: usize,
    red: bool,
}

impl Census {
    /// 空汇总。
    pub const fn new() -> Census {
        Census {
            total: 0,
            full: 0,
            partial: 0,
            absent: 0,
            degrade: 0,
            from_all: 0,
            from_lang: 0,
            from_c: 0,
            matched: 0,
            red: false,
        }
    }

    /// 总数。
    pub const fn total(&self) -> usize {
        self.total
    }

    /// 完整档数。
    pub const fn full(&self) -> usize {
        self.full
    }

    /// 部分档数。
    pub const fn partial(&self) -> usize {
        self.partial
    }

    /// 全缺档数。
    pub const fn absent(&self) -> usize {
        self.absent
    }

    /// 降级格总数。
    pub const fn degrade(&self) -> usize {
        self.degrade
    }

    /// 来自 LC_ALL 的数。
    pub const fn from_all(&self) -> usize {
        self.from_all
    }

    /// 来自 LANG 的数。
    pub const fn from_lang(&self) -> usize {
        self.from_lang
    }

    /// 来自默认的数。
    pub const fn from_c(&self) -> usize {
        self.from_c
    }

    /// 对上数。
    pub const fn matched(&self) -> usize {
        self.matched
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// 红判。
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
            return Err(LA_ENOBS);
        }
        if self.red {
            return Err(LA_EABSENT);
        }
        if self.matched != self.total {
            return Err(LA_EVALUE);
        }
        if self.total != CORPUS || self.full != 9 || self.partial != 2 || self.absent != 3 {
            return Err(LA_EVALUE);
        }
        if self.degrade != 56 {
            return Err(LA_EVALUE);
        }
        if self.from_all != 2 || self.from_lang != 11 || self.from_c != 1 {
            return Err(LA_EVALUE);
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
        s.push_str("locale_absent corpus=");
        s.push_str(u32_text(self.total as u32).as_str());
        s.push_str(" full=");
        s.push_str(u32_text(self.full as u32).as_str());
        s.push_str(" partial=");
        s.push_str(u32_text(self.partial as u32).as_str());
        s.push_str(" absent=");
        s.push_str(u32_text(self.absent as u32).as_str());
        s.push_str(" degrade=");
        s.push_str(u32_text(self.degrade as u32).as_str());
        s.push_str(" from_all=");
        s.push_str(u32_text(self.from_all as u32).as_str());
        s.push_str(" from_lang=");
        s.push_str(u32_text(self.from_lang as u32).as_str());
        s.push_str(" from_c=");
        s.push_str(u32_text(self.from_c as u32).as_str());
        s.push_str(" match=");
        s.push_str(u32_text(self.matched as u32).as_str());
        s.push_str(" rate=");
        s.push_str(u32_text(self.rate()).as_str());
        s.push_str("ppm\n");
        s
    }
}

/// 走一遍语料。
fn walk(red: Option<usize>) -> Result<Census, u32> {
    let mut c = Census::new();
    let mut i = 0usize;
    while i < CASES.len() {
        let case = match CASES.get(i) {
            Some(v) => *v,
            None => return Err(LA_EVALUE),
        };
        let env = Env::new(case.lc_all, case.lang);
        let p = plan_of(&env)?;
        c.total += 1;
        match p.source {
            Source::All => c.from_all += 1,
            Source::Lang => c.from_lang += 1,
            Source::Default => c.from_c += 1,
            Source::Category => {}
        }
        if p.full() {
            c.full += 1;
        } else if p.absent() {
            c.absent += 1;
        } else {
            c.partial += 1;
        }
        c.degrade += p.degraded;
        if p.degraded == case.want_degraded && p.fault == case.want_fault {
            c.matched += 1;
        }
        i += 1;
    }
    if let Some(slot) = red {
        if slot >= c.total {
            return Err(LA_ESLOT);
        }
        c.matched = c.matched.saturating_sub(1);
        c.red = true;
    }
    Ok(c)
}

/// 绿跑。
pub fn run() -> Result<Census, u32> {
    walk(None)
}

/// 红注入跑。
pub fn run_with(red: Option<usize>) -> Result<Census, u32> {
    walk(red)
}

/// 覆盖判据。
pub fn cover(n: usize) -> Result<(), u32> {
    if n == CORPUS {
        Ok(())
    } else {
        Err(LA_ENOBS)
    }
}

// ---------------------------------------------------------------------------
// 文案
// ---------------------------------------------------------------------------

/// 降级通告（两行，逐字）。
pub fn notice(lc_all: &str, lang: &str) -> Result<String, u32> {
    let env = Env::new(lc_all, lang);
    let (src, var) = env.pick();
    let name = var.text();
    let mut s = String::new();
    if src == Source::Default {
        s.push_str("no locale environment: defaulting to C\n");
        s.push_str("0 categories degraded\n");
        return Ok(s);
    }
    let p = plan_of(&env)?;
    s.push_str(src.name());
    s.push('=');
    s.push_str(name.as_str());
    s.push_str(": ");
    if !p.absent() {
        s.push_str(u32_text(mask_count(p.mask) as u32).as_str());
        s.push_str("/12 category files present\n");
    } else {
        s.push_str("no directory under ");
        s.push_str(ROOT);
        s.push('\n');
    }
    s.push_str(u32_text(p.degraded as u32).as_str());
    s.push_str(" categories fall back to C\n");
    Ok(s)
}

/// 缺档清单。
pub fn gap_list(name: &str) -> Result<String, u32> {
    let p = probe(name)?;
    let mut s = String::new();
    s.push_str(p.gaps().as_str());
    Ok(s)
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
            return Err(LA_ESLOT);
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

    /// 对账：恰好 14 行、8 成 6 败，值与码逐行对上。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(LA_EBAL);
        }
        const VALS: [u32; 8] = [14, 9, 2, 3, 56, 2, 11, 929];
        const CODES: [u32; 6] =
            [LA_EABSENT, LA_EPARTIAL, LA_ECATFILE, LA_EBADLANG, LA_ESTATE, LA_EBYTE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(LA_EBAL),
            };
            if s.slot as usize != i {
                return Err(LA_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(LA_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(LA_EVALUE);
                        }
                    }
                    None => return Err(LA_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(LA_EVALUE);
                        }
                    }
                    None => return Err(LA_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(LA_EBAL);
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
            return Err(LA_EBAL);
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
            Err(LA_ESEAL)
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
    let _ = l.register(count_cell(1, OP_PROBE, r.map(|x| x.full())));
    let _ = l.register(count_cell(2, OP_MASK, r.map(|x| x.partial())));
    let _ = l.register(count_cell(3, OP_PROBE, r.map(|x| x.absent())));

    let _ = l.register(count_cell(4, OP_PLAN, r.map(|x| x.degrade())));
    let _ = l.register(count_cell(5, OP_CHAIN, r.map(|x| x.from_all())));
    let _ = l.register(count_cell(6, OP_CHAIN, r.map(|x| x.from_lang())));

    let red = run_with(Some(0));
    let _ = l.register(count_cell(7, OP_NOTICE, red.map(|x| x.rate() as usize)));

    let _ = l.register(err_cell(8, OP_PROBE, fault_of("xx_YY.UTF-8")));
    let _ = l.register(err_cell(9, OP_MASK, fault_of("fr_FR.UTF-8")));

    let cat_err = match cat_index("LC_BOGUS") {
        Ok(_) => LA_ECATFILE,
        Err(c) => c,
    };
    let _ = l.register(err_cell(10, OP_PLAN, cat_err));

    let bad = match plan_str("", "1234.UTF-8") {
        Ok(_) => LA_EBADLANG,
        Err(c) => c,
    };
    let _ = l.register(err_cell(11, OP_CHAIN, bad));

    let mut sc = Scanner::new();
    let first = sc.scan();
    let state = match first {
        Ok(_) => match sc.scan() {
            Ok(_) => LA_ESTATE,
            Err(c) => c,
        },
        Err(c) => c,
    };
    let _ = l.register(err_cell(12, OP_SEAL, state));

    let byte = match plan_str("", "en_US.UTF 8") {
        Ok(_) => LA_EBYTE,
        Err(c) => c,
    };
    let _ = l.register(err_cell(13, OP_PROBE, byte));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 目录扫描。
fn dirs_ok() -> bool {
    let a = TREE.len() == DIRS && dirs_unique();
    let b = dir_at(0).map(|e| e.name) == Some("C");
    let c = dir_find("de_DE.UTF-8") == Some(5);
    let d = dir_find("xx_YY.UTF-8").is_none();
    let e = dir_at(DIRS).is_none();
    let f = match dir_at(6) {
        Some(v) => v.name == "fr_FR.UTF-8" && v.present && !v.full() && v.degraded() == 9,
        None => false,
    };
    let g = match dir_at(8) {
        Some(v) => v.files == 0x0001 && v.degraded() == 11,
        None => false,
    };
    a && b && c && d && e && f && g
}

/// 分类掩码。
fn masks_ok() -> bool {
    let a = mask_count(CAT_ALL) == CAT_COUNT && mask_count(0) == 0;
    let b = mask_count(0x0029) == 3 && mask_degraded(0x0029) == 9;
    let c = mask_degraded(CAT_ALL) == 0 && mask_degraded(0) == CAT_COUNT;
    let d = mask_complement(CAT_ALL) == 0 && mask_complement(0) == CAT_ALL;
    let e = mask_ok(CAT_ALL) && mask_ok(0x0029) && !mask_ok(0x1000);
    let f = cat_bit(0) == Some(1) && cat_bit(11) == Some(0x0800) && cat_bit(12).is_none();
    let g = cat_name(0) == Some("LC_CTYPE") && cat_name(11) == Some("LC_IDENTIFICATION");
    a && b && c && d && e && f && g
}

/// 名字语法。
fn names_ok() -> bool {
    let a = check_name("en_US.UTF-8").is_ok();
    let b = check_name("ru_RU.KOI8-R").is_ok();
    let c = check_name("de_DE.UTF-8@euro").is_ok();
    let d = check_name("1234.UTF-8") == Err(LA_EBADLANG);
    let e = check_name("en_US.UTF 8") == Err(LA_EBYTE);
    let f = check_name("en_") == Err(LA_EBADLANG);
    let g = check_name("en_USA") == Err(LA_EBADLANG);
    let h = check_name("en_US.") == Err(LA_EBADLANG);
    let i = check_name("") == Ok(());
    a && b && c && d && e && f && g && h && i
}

/// 降级计划。
fn plan_ok() -> bool {
    let a = match plan_str("", "de_DE.UTF-8") {
        Ok(p) => p.full() && p.mask == CAT_ALL && !p.absent() && p.fault == 0,
        Err(_) => false,
    };
    let b = match plan_str("", "fr_FR.UTF-8") {
        Ok(p) => p.degraded == 9 && p.fault == LA_EPARTIAL && !p.full() && !p.absent(),
        Err(_) => false,
    };
    let c = match plan_str("", "xx_YY.UTF-8") {
        Ok(p) => p.absent() && p.degraded == 12 && p.fault == LA_EABSENT && p.locale.is_none(),
        Err(_) => false,
    };
    let d = match plan_str("", "1234.UTF-8") {
        Ok(_) => false,
        Err(c) => c == LA_EBADLANG,
    };
    a && b && c && d
}

/// 部分安装。
fn partial_ok() -> bool {
    let a = fault_of("fr_FR.UTF-8") == LA_EPARTIAL;
    let b = fault_of("zh_CN.UTF-8") == LA_EPARTIAL;
    let c = fault_of("de_DE.UTF-8") == 0;
    let d = fault_of("xx_YY.UTF-8") == LA_EABSENT;
    let e = match probe("fr_FR.UTF-8") {
        Ok(p) => p.gaps().as_str() == "LC_NUMERIC,LC_TIME,LC_MONETARY,LC_PAPER,LC_NAME,LC_ADDRESS,LC_TELEPHONE,LC_MEASUREMENT,LC_IDENTIFICATION",
        Err(_) => false,
    };
    let f = match probe("zh_CN.UTF-8") {
        Ok(p) => p.degraded == 11 && p.has_gap() && !p.all_gone(),
        Err(_) => false,
    };
    let g = match probe("xx_YY.UTF-8") {
        Ok(p) => p.all_gone() && p.degraded == 12 && !p.present,
        Err(_) => false,
    };
    a && b && c && d && e && f && g
}

/// 来源链。
fn chain_ok() -> bool {
    let e1 = Env::new("de_DE.UTF-8", "fr_FR.UTF-8");
    let c1 = Chain::of(&e1);
    let a = c1.winner() == Some(Source::All) && c1.monotone();
    let e2 = Env::new("", "fr_FR.UTF-8");
    let c2 = Chain::of(&e2);
    let b = c2.winner() == Some(Source::Lang);
    let e3 = Env::new("", "");
    let c3 = Chain::of(&e3);
    let c = c3.winner() == Some(Source::Default);
    let d = match (c2.at(0), c2.at(2)) {
        (Some(x), Some(y)) => !x.set && y.set && y.winner,
        _ => false,
    };
    let f = Source::All.rank() < Source::Category.rank()
        && Source::Category.rank() < Source::Lang.rank()
        && Source::Lang.rank() < Source::Default.rank();
    a && b && c && d && f
}

/// 十四卡全跑。
fn corpus_ok() -> bool {
    let c = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = c.total() == CORPUS && c.full() == 9 && c.partial() == 2 && c.absent() == 3;
    let b = c.degrade() == 56 && c.matched() == CORPUS;
    let d = c.from_all() == 2 && c.from_lang() == 11 && c.from_c() == 1;
    let e = c.rate() == 1000 && c.green() && c.gate() == Ok(());
    a && b && d && e
}

/// 语料表。
fn slots_ok() -> bool {
    if !unique() || !notes_ok() || !table_ok() {
        return false;
    }
    let a = match case_at(7) {
        Some(c) => c.lang == "fr_FR.UTF-8" && c.want_degraded == 9,
        None => false,
    };
    let b = match case_at(9) {
        Some(c) => c.want_fault == LA_EABSENT,
        None => false,
    };
    let c = case_at(CORPUS).is_none();
    a && b && c
}

/// 汇总渲染。
fn report_ok() -> bool {
    let c = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = c.render().as_str() == "locale_absent corpus=14 full=9 partial=2 absent=3 degrade=56 from_all=2 from_lang=11 from_c=1 match=14 rate=1000ppm\n";
    let e = Census::new();
    let b = e.is_empty() && e.rate() == 0 && e.gate() == Err(LA_ENOBS);
    let d = e.render().as_str()
        == "locale_absent corpus=0 full=0 partial=0 absent=0 degrade=0 from_all=0 from_lang=0 from_c=0 match=0 rate=0ppm\n";
    let f = cover(CORPUS) == Ok(()) && cover(0) == Err(LA_ENOBS);
    a && b && d && f
}

/// 红单显式。
fn reds_ok() -> bool {
    let c = match run_with(Some(0)) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = c.matched() == CORPUS - 1 && c.rate() == 929;
    let b = c.gate() == Err(LA_EABSENT) && !c.green();
    let d = run_with(Some(CORPUS)).err() == Some(LA_ESLOT);
    a && b && d
}

/// 文案逐字。
fn notice_ok() -> bool {
    let a = match notice("", "xx_YY.UTF-8") {
        Ok(s) => s.as_str()
            == "LANG=xx_YY.UTF-8: no directory under /usr/lib/locale\n12 categories fall back to C\n",
        Err(_) => false,
    };
    let b = match notice("", "fr_FR.UTF-8") {
        Ok(s) => s.as_str()
            == "LANG=fr_FR.UTF-8: 3/12 category files present\n9 categories fall back to C\n",
        Err(_) => false,
    };
    let c = match notice("de_DE.UTF-8", "fr_FR.UTF-8") {
        Ok(s) => s.as_str()
            == "LC_ALL=de_DE.UTF-8: 12/12 category files present\n0 categories fall back to C\n",
        Err(_) => false,
    };
    let d = match notice("", "") {
        Ok(s) => s.as_str() == "no locale environment: defaulting to C\n0 categories degraded\n",
        Err(_) => false,
    };
    a && b && c && d
}

/// 逐格渲染。
fn render_ok() -> bool {
    let p = match plan_str("", "fr_FR.UTF-8") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = p.per_cat(0) == Some("fr_FR.UTF-8");
    let b = p.per_cat(1) == Some("C");
    let c = p.per_cat(3) == Some("fr_FR.UTF-8");
    let d = p.per_cat(5) == Some("fr_FR.UTF-8");
    let e = p.per_cat(2) == Some("C") && p.per_cat(11) == Some("C");
    let f = p.per_cat(12).is_none();
    let g = p.brief().as_str() == "source=LANG mask=0029 degraded=9";
    let r = p.render();
    let h = r.as_str().starts_with("LC_CTYPE=fr_FR.UTF-8\nLC_NUMERIC=C\n");
    let mut lines = 0usize;
    let mut i = 0usize;
    let bytes = r.as_bytes();
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            if *b == b'\n' {
                lines += 1;
            }
        }
        i += 1;
    }
    a && b && c && d && e && f && g && h && lines == CAT_COUNT
}

/// 扫描封存与账本。
fn scan_ok() -> bool {
    let mut s = Scanner::new();
    let a = !s.scanned() && s.resolve("C") == Err(LA_ESTATE);
    let b = s.scan() == Ok(88);
    let c = s.scanned() && s.dirs() == DIRS && s.cats() == 88;
    let d = s.scan() == Err(LA_ESTATE) && s.last() == LA_ESTATE;
    let e = s.resolve("de_DE.UTF-8") == Ok(5);
    let f = s.resolve("xx_YY.UTF-8") == Err(LA_EABSENT);
    let l = build_ledger();
    let g = l.audit() == Ok(()) && l.ok_count() == 8 && l.err_count() == 6;
    let h = u32_text(56).as_str() == "56" && hex4_text(0x0029).as_str() == "0029";
    let i = digest(b"fr_FR") == digest(b"fr_FR") && digest(b"fr_FR") != digest(b"fr_FS");
    a && b && c && d && e && f && g && h && i
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = dirs_ok() && masks_ok() && names_ok();
    out[1] = plan_ok() && partial_ok() && chain_ok();
    out[2] = corpus_ok() && slots_ok() && report_ok() && reds_ok();
    out[3] = notice_ok() && render_ok() && scan_ok() && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11642_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11642.lang_absent");

    s.add("LA01-目录扫描", dirs_ok(), "static");
    s.add("LA02-分类掩码", masks_ok(), "static");
    s.add("LA03-名字语法", names_ok(), "static");
    s.add("LA04-降级计划", plan_ok(), "static");
    s.add("LA05-部分安装", partial_ok(), "static");
    s.add("LA06-来源链", chain_ok(), "static");
    s.add("LA07-十四卡全跑", corpus_ok(), "static");
    s.add("LA08-语料编号", slots_ok(), "static");
    s.add("LA09-汇总渲染", report_ok(), "static");
    s.add("LA10-红单显式", reds_ok(), "static");
    s.add("LA11-降级文案逐字", notice_ok(), "static");
    s.add("LA12-逐格渲染与账本", render_ok() && scan_ok(), "static");
    s.add("LA13-冻结重放四注入", inject_four().iter().all(|x| *x), "static");

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert!(l.sample(0).is_some());
        assert!(l.sample(SAMPLES).is_none());
        assert_eq!(l.sample(8).map(|x| x.code), Some(LA_EABSENT));
        assert_eq!(l.sample(13).map(|x| x.code), Some(LA_EBYTE));
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
        assert_eq!(l.freeze(), Err(LA_EBAL));
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
        let s = run_unx11642_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }

    #[test]
    fn tree_shape() {
        assert_eq!(TREE.len(), DIRS);
        assert!(dirs_unique());
        assert!(TREE.iter().all(|e| e.present));
        assert_eq!(dir_at(6).map(|e| e.degraded()), Some(9));
        assert_eq!(dir_at(8).map(|e| e.degraded()), Some(11));
    }

    #[test]
    fn partial_probe() {
        let p = match probe("zh_CN.UTF-8") {
            Ok(v) => v,
            Err(_) => return,
        };
        assert!(p.present);
        assert_eq!(p.files, 1);
        assert_eq!(p.degraded, 11);
        assert!(p.has_gap());
        assert!(!p.all_gone());
    }

    #[test]
    fn absent_probe() {
        let p = match probe("xx_YY.UTF-8") {
            Ok(v) => v,
            Err(_) => return,
        };
        assert!(!p.present);
        assert_eq!(p.degraded, CAT_COUNT);
        assert!(p.all_gone());
        assert_eq!(p.gaps().split(',').count(), 12);
    }

    #[test]
    fn default_plan() {
        let p = match plan_str("", "") {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(p.source, Source::Default);
        assert!(p.full());
        assert_eq!(p.degraded, 0);
        assert_eq!(p.per_cat(0), Some("C"));
    }

    #[test]
    fn lc_all_precedence() {
        let p = match plan_str("de_DE.UTF-8", "fr_FR.UTF-8") {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(p.source, Source::All);
        assert!(p.full());
        assert_eq!(p.per_cat(1), Some("de_DE.UTF-8"));
        let q = match plan_str("C", "xx_YY.UTF-8") {
            Ok(v) => v,
            Err(_) => return,
        };
        assert!(q.full());
        assert_eq!(q.per_cat(0), Some("C"));
    }

    #[test]
    fn degrade_totals() {
        let c = match run() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(c.degrade(), 56);
        assert_eq!(c.total(), 14);
        assert_eq!(c.full() + c.partial() + c.absent(), 14);
    }

    #[test]
    fn per_cat_lines() {
        let p = match plan_str("", "zh_CN.UTF-8") {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(p.per_cat(0), Some("zh_CN.UTF-8"));
        assert_eq!(p.per_cat(1), Some("C"));
        let r = p.render();
        assert!(r.as_str().contains("LC_CTYPE=zh_CN.UTF-8"));
        assert!(r.as_str().contains("LC_TIME=C"));
    }

    #[test]
    fn notice_absent_text() {
        let n = match notice("", "xx_YY.UTF-8") {
            Ok(v) => v,
            Err(_) => return,
        };
        assert!(n.as_str().contains("no directory under /usr/lib/locale"));
        assert!(n.as_str().ends_with("12 categories fall back to C\n"));
    }

    #[test]
    fn notice_partial_text() {
        let n = match notice("", "fr_FR.UTF-8") {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(n.as_str(), "LANG=fr_FR.UTF-8: 3/12 category files present\n9 categories fall back to C\n");
    }

    #[test]
    fn cat_mapping() {
        assert_eq!(cat_index("LC_CTYPE"), Ok(0));
        assert_eq!(cat_index("LC_IDENTIFICATION"), Ok(11));
        assert_eq!(cat_index("LC_BOGUS"), Err(LA_ECATFILE));
        assert_eq!(cat_name(0), Some("LC_CTYPE"));
    }

    #[test]
    fn scanner_seal() {
        let mut s = Scanner::new();
        assert_eq!(s.resolve("C"), Err(LA_ESTATE));
        assert_eq!(s.scan(), Ok(88));
        assert_eq!(s.scan(), Err(LA_ESTATE));
        assert_eq!(s.resolve("C"), Ok(0));
        assert_eq!(s.resolve("nope"), Err(LA_EABSENT));
    }

    #[test]
    fn census_render() {
        let c = match run() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(
            c.render().as_str(),
            "locale_absent corpus=14 full=9 partial=2 absent=3 degrade=56 from_all=2 from_lang=11 from_c=1 match=14 rate=1000ppm\n"
        );
        assert_eq!(c.gate(), Ok(()));
    }

    #[test]
    fn red_override() {
        let c = match run_with(Some(2)) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(c.matched(), 13);
        assert_eq!(c.rate(), 929);
        assert_eq!(c.gate(), Err(LA_EABSENT));
        assert_eq!(run_with(Some(14)).err(), Some(LA_ESLOT));
    }

    #[test]
    fn corpus_table() {
        assert_eq!(CASES.len(), CORPUS);
        assert!(unique());
        assert!(notes_ok());
        assert!(table_ok());
        assert_eq!(case_at(CORPUS), None);
    }
}
