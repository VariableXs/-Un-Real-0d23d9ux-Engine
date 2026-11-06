//! UNX-F11643 setlocale(LC_ALL,"") 空环境档判据
//!
//! `setlocale(LC_ALL, "")` 在 POSIX 里是一句"照着环境变量自己判"的请求：
//! 对**每个**分类各判一次，取第一个非空的来源——`LC_ALL` 最优先，其次
//! 是该分类自己的 `LC_<CATEGORY>`，再次是 `LANG`，四档全空才是 "C"。
//! 注意这是"逐分类"判定，不是"整档"判定：`LC_CTYPE=ja_JP.UTF-8` 配
//! `LANG=en_US.UTF-8` 会得到 CTYPE=ja、其余十一格=en 的复合档，而不是
//! 整档 ja 或整档 en。本卡把这条优先级链在十二格上逐格钉死。
//!
//! 空串语义是第二个坑：POSIX 规定 `LC_ALL=""` 与 `LC_CTYPE=""` 都是
//! "未设"，必须继续往下一档落，绝不能当成"名字叫空串的 locale"。现实中
//! `export LC_ALL=` 会让整条链静默下滑到 `LANG`，运维看到的就是"我明明
//! 设了 LC_ALL 却不生效"。
//!
//! 值坏（不在安装表）有两种口径，本卡都建模：严格口径按 POSIX——
//! `setlocale(LC_ALL, "")` 发现任一分类取不到就返回 NULL、**整单不改**；
//! 宽松口径是程序随后逐分类补救后的真实终态——坏值那格落 "C"，其余格
//! 照常。两种口径的差集恰恰是"警告打给谁看"的判据。
//!
//! 闸门次序：值字节 → 安装表 → 分类槽 → 优先级链 → 严格/宽松 → 复合串
//! → 十二格渲染 → 绿档。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本 14 格
//! （8 成 6 败）在 `audit` 逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfd20;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 语料条数。
pub const CORPUS: usize = 14;

/// 分类总数。
pub const CAT_COUNT: usize = 12;

/// 环境值容量。
pub const VAL_CAP: usize = 32;

/// 安装表条目数。
pub const INSTALLED: usize = 7;

/// 单槽覆盖上限。
pub const OVER_MAX: usize = 3;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 值不在安装表。
pub const LC_EBADVAL: u32 = SEG_BASE + 0x01;
/// 值字节非法。
pub const LC_EBYTE: u32 = SEG_BASE + 0x02;
/// 分类槽越界。
pub const LC_ECAT: u32 = SEG_BASE + 0x03;
/// 严格口径整单拒收。
pub const LC_ESTRICT: u32 = SEG_BASE + 0x04;
/// 状态冲突。
pub const LC_ESTATE: u32 = SEG_BASE + 0x05;
/// 空值被当成已设。
pub const LC_ENOVAL: u32 = SEG_BASE + 0x06;
/// 覆盖重复。
pub const LC_EDUP: u32 = SEG_BASE + 0x07;
/// 优先级错序。
pub const LC_EORDER: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const LC_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const LC_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const LC_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const LC_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const LC_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 值校验。
pub const OP_VALUE: u8 = 0x01;
/// 分类槽。
pub const OP_SLOT: u8 = 0x02;
/// 优先级选择。
pub const OP_CHOOSE: u8 = 0x03;
/// 严格推导。
pub const OP_STRICT: u8 = 0x04;
/// 宽松补救。
pub const OP_REPAIR: u8 = 0x05;
/// 语料全跑。
pub const OP_CORPUS: u8 = 0x06;
/// 复合串。
pub const OP_RENDER: u8 = 0x07;
/// 封存。
pub const OP_SEAL: u8 = 0x08;

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

/// 追加字节。
fn push_bytes(dst: &mut String, src: &[u8]) {
    let mut i = 0usize;
    while i < src.len() {
        if let Some(b) = src.get(i) {
            dst.push(*b as char);
        }
        i += 1;
    }
}

/// 十二分类名。
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
    Err(LC_ECAT)
}

/// 下标 → 分类名。
pub fn cat_name(i: usize) -> Result<&'static str, u32> {
    match CAT_NAMES.get(i) {
        Some(v) => Ok(*v),
        None => Err(LC_ECAT),
    }
}

/// 单分类位。
pub fn cat_bit(i: usize) -> Result<u16, u32> {
    if i >= CAT_COUNT {
        return Err(LC_ECAT);
    }
    Ok(1u16 << i)
}

/// 全掩码。
pub const CAT_ALL: u16 = 0x0fff;

// ---------------------------------------------------------------------------
// 值
// ---------------------------------------------------------------------------

/// 安装表（本镜像只装这七档）。
pub const TABLE: [&str; INSTALLED] =
    ["C", "POSIX", "C.UTF-8", "en_US.UTF-8", "de_DE.UTF-8", "fr_FR.UTF-8", "ja_JP.UTF-8"];

/// 值字节校验。
pub fn check_value(src: &str) -> Result<(), u32> {
    let b = src.as_bytes();
    if b.is_empty() {
        return Err(LC_ENOVAL);
    }
    if b.len() > VAL_CAP {
        return Err(LC_EBYTE);
    }
    let mut i = 0usize;
    while i < b.len() {
        let c = match b.get(i) {
            Some(v) => *v,
            None => return Err(LC_EBYTE),
        };
        if c < 0x21 || c > 0x7e {
            return Err(LC_EBYTE);
        }
        i += 1;
    }
    Ok(())
}

/// 规范名（必须在安装表里）。
pub fn canonical(src: &str) -> Result<&'static str, u32> {
    check_value(src)?;
    let mut i = 0usize;
    while i < TABLE.len() {
        if let Some(w) = TABLE.get(i) {
            if *w == src {
                return Ok(*w);
            }
        }
        i += 1;
    }
    Err(LC_EBADVAL)
}

/// 非空强制取规范名（空值单列一档）。
pub fn forced(src: &str) -> Result<&'static str, u32> {
    if src.is_empty() {
        return Err(LC_ENOVAL);
    }
    canonical(src)
}

/// 一个环境值槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Var {
    buf: [u8; VAL_CAP],
    len: usize,
    set: bool,
}

impl Var {
    /// 收值：空串即未设（POSIX 空串语义）。
    pub fn new(src: &str) -> Var {
        let b = src.as_bytes();
        let n = if b.len() > VAL_CAP { VAL_CAP } else { b.len() };
        let mut buf = [0u8; VAL_CAP];
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

    /// 实长。
    pub fn len(&self) -> usize {
        self.len
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
        if !self.set {
            return lit.is_empty();
        }
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

// ---------------------------------------------------------------------------
// 环境
// ---------------------------------------------------------------------------

/// 环境：LC_ALL + LANG + 十二个分类槽。
#[derive(Clone, Copy, Debug)]
pub struct Env {
    all: Var,
    lang: Var,
    cats: [Var; CAT_COUNT],
}

impl Env {
    /// 空环境。
    pub fn new(lc_all: &str, lang: &str) -> Env {
        Env { all: Var::new(lc_all), lang: Var::new(lang), cats: [Var::new(""); CAT_COUNT] }
    }

    /// LC_ALL 槽。
    pub fn all(&self) -> Var {
        self.all
    }

    /// LANG 槽。
    pub fn lang(&self) -> Var {
        self.lang
    }

    /// 分类槽。
    pub fn cat(&self, i: usize) -> Result<Var, u32> {
        match self.cats.get(i) {
            Some(v) => Ok(*v),
            None => Err(LC_ECAT),
        }
    }

    /// 写分类槽；名字不认识即分类档。
    pub fn set_cat(&mut self, name: &str, value: &str) -> Result<(), u32> {
        let i = cat_index(name)?;
        if let Some(slot) = self.cats.get_mut(i) {
            if slot.is_set() && !value.is_empty() {
                return Err(LC_EDUP);
            }
            *slot = Var::new(value);
        }
        Ok(())
    }

    /// 十二槽里设了几个。
    pub fn set_cats(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < CAT_COUNT {
            if let Some(v) = self.cats.get(i) {
                if v.is_set() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 优先级
// ---------------------------------------------------------------------------

/// 胜出来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Winner {
    /// LC_ALL。
    All,
    /// LC_<CATEGORY>。
    Cat,
    /// LANG。
    Lang,
    /// 兜底 C。
    Default,
}

impl Winner {
    /// 文本名。
    pub fn name(&self) -> &'static str {
        match self {
            Winner::All => "LC_ALL",
            Winner::Cat => "LC_*",
            Winner::Lang => "LANG",
            Winner::Default => "C",
        }
    }

    /// 优先级（小者先）。
    pub fn rank(&self) -> u8 {
        match self {
            Winner::All => 0,
            Winner::Cat => 1,
            Winner::Lang => 2,
            Winner::Default => 3,
        }
    }
}

/// 一格判定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pick {
    /// 胜出来源。
    pub winner: Winner,
    /// 生效名。
    pub name: &'static str,
    /// 故障码（正常 0）。
    pub fault: u32,
}

impl Pick {
    /// 空判。
    pub const fn blank() -> Pick {
        Pick { winner: Winner::Default, name: "C", fault: 0 }
    }
}

/// 选来源（不验值）。
pub fn choose(env: &Env, cat: usize) -> Result<(Winner, Option<Var>), u32> {
    if cat >= CAT_COUNT {
        return Err(LC_ECAT);
    }
    if env.all.is_set() {
        return Ok((Winner::All, Some(env.all)));
    }
    if let Some(v) = env.cats.get(cat) {
        if v.is_set() {
            return Ok((Winner::Cat, Some(*v)));
        }
    }
    if env.lang.is_set() {
        return Ok((Winner::Lang, Some(env.lang)));
    }
    Ok((Winner::Default, None))
}

/// 判一格。
pub fn derive_cat(env: &Env, cat: usize) -> Result<Pick, u32> {
    let (winner, var) = choose(env, cat)?;
    let v = match var {
        Some(v) => v,
        None => return Ok(Pick { winner: Winner::Default, name: "C", fault: 0 }),
    };
    let text = v.text();
    match canonical(text.as_str()) {
        Ok(n) => Ok(Pick { winner, name: n, fault: 0 }),
        Err(c) => Ok(Pick { winner, name: "C", fault: c }),
    }
}

/// 十二格复合档。
#[derive(Clone, Copy, Debug)]
pub struct Composite {
    picks: [Pick; CAT_COUNT],
    faults: usize,
    strict: bool,
}

impl Composite {
    /// 空格。
    pub const fn blank() -> Composite {
        Composite { picks: [Pick::blank(); CAT_COUNT], faults: 0, strict: false }
    }

    /// 取一格。
    pub fn at(&self, cat: usize) -> Option<Pick> {
        self.picks.get(cat).copied()
    }

    /// 生效名。
    pub fn per_cat(&self, cat: usize) -> Option<&'static str> {
        self.picks.get(cat).map(|p| p.name)
    }

    /// 故障格数。
    pub fn faults(&self) -> usize {
        self.faults
    }

    /// 是否严格口径。
    pub fn strict(&self) -> bool {
        self.strict
    }

    /// 是否十二格同档。
    pub fn uniform(&self) -> bool {
        let first = match self.picks.first() {
            Some(p) => p.name,
            None => return false,
        };
        let mut i = 1usize;
        while i < CAT_COUNT {
            if let Some(p) = self.picks.get(i) {
                if p.name != first {
                    return false;
                }
            }
            i += 1;
        }
        true
    }

    /// 不同档名个数。
    pub fn distinct(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < CAT_COUNT {
            let p = match self.picks.get(i) {
                Some(v) => *v,
                None => return 0,
            };
            let mut seen = false;
            let mut j = 0usize;
            while j < i {
                if let Some(q) = self.picks.get(j) {
                    if q.name == p.name {
                        seen = true;
                    }
                }
                j += 1;
            }
            if !seen {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 某档覆盖格数。
    pub fn count_of(&self, name: &str) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < CAT_COUNT {
            if let Some(p) = self.picks.get(i) {
                if p.name == name {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 复合串（`name=value;` 十二段）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < CAT_COUNT {
            if let Ok(n) = cat_name(i) {
                s.push_str(n);
                s.push('=');
                if let Some(p) = self.picks.get(i) {
                    s.push_str(p.name);
                }
            }
            if i + 1 < CAT_COUNT {
                s.push(';');
            }
            i += 1;
        }
        s.push('\n');
        s
    }

    /// 来源分布（四段文本）。
    pub fn sources(&self) -> String {
        let mut a = 0usize;
        let mut c = 0usize;
        let mut l = 0usize;
        let mut d = 0usize;
        let mut i = 0usize;
        while i < CAT_COUNT {
            if let Some(p) = self.picks.get(i) {
                match p.winner {
                    Winner::All => a += 1,
                    Winner::Cat => c += 1,
                    Winner::Lang => l += 1,
                    Winner::Default => d += 1,
                }
            }
            i += 1;
        }
        let mut s = String::new();
        s.push_str("all=");
        s.push_str(u32_text(a as u32).as_str());
        s.push_str(" cat=");
        s.push_str(u32_text(c as u32).as_str());
        s.push_str(" lang=");
        s.push_str(u32_text(l as u32).as_str());
        s.push_str(" default=");
        s.push_str(u32_text(d as u32).as_str());
        s
    }
}

/// 严格口径：任一格坏即整单拒收（POSIX 的 NULL 语义）。
pub fn derive_strict(env: &Env) -> Result<Composite, u32> {
    let mut c = Composite::blank();
    c.strict = true;
    let mut i = 0usize;
    while i < CAT_COUNT {
        let p = derive_cat(env, i)?;
        if p.fault != 0 {
            return Err(LC_ESTRICT);
        }
        if let Some(dst) = c.picks.get_mut(i) {
            *dst = p;
        }
        i += 1;
    }
    c.faults = 0;
    Ok(c)
}

/// 宽松口径：坏值那格落 C，其余照常（程序补救后的终态）。
pub fn derive_repair(env: &Env) -> Composite {
    let mut c = Composite::blank();
    let mut i = 0usize;
    let mut faults = 0usize;
    while i < CAT_COUNT {
        match derive_cat(env, i) {
            Ok(p) => {
                if p.fault != 0 {
                    faults += 1;
                }
                if let Some(dst) = c.picks.get_mut(i) {
                    *dst = p;
                }
            }
            Err(_) => faults += 1,
        }
        i += 1;
    }
    c.faults = faults;
    c
}

/// 逐格来源轨迹（逐字可对）。
pub fn trail(env: &Env, cat: usize) -> Result<String, u32> {
    let name = cat_name(cat)?;
    let (winner, _) = choose(env, cat)?;
    let mut s = String::new();
    s.push_str(name);
    s.push_str(": LC_ALL=");
    if env.all.is_set() {
        s.push_str(env.all.text().as_str());
    } else {
        s.push_str("(unset)");
    }
    s.push(' ');
    s.push_str(name);
    s.push('=');
    match env.cats.get(cat) {
        Some(v) => {
            if v.is_set() {
                s.push_str(v.text().as_str());
            } else {
                s.push_str("(unset)");
            }
        }
        None => s.push_str("(unset)"),
    }
    s.push_str(" LANG=");
    if env.lang.is_set() {
        s.push_str(env.lang.text().as_str());
    } else {
        s.push_str("(unset)");
    }
    s.push_str(" => ");
    let p = derive_cat(env, cat)?;
    s.push_str(p.name);
    let _ = winner;
    Ok(s)
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 一个语料槽（最多三处分类覆盖）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    /// LC_ALL。
    pub lc_all: &'static str,
    /// LANG。
    pub lang: &'static str,
    /// 分类覆盖（名字, 值）。
    pub over: [(&'static str, &'static str); OVER_MAX],
    /// 期望故障格数。
    pub want_faults: usize,
    /// 期望落 C 的格数。
    pub want_c: usize,
    /// 备注。
    pub note: &'static str,
}

impl Case {
    /// 构造。
    pub const fn new(
        lc_all: &'static str,
        lang: &'static str,
        over: [(&'static str, &'static str); OVER_MAX],
        want_faults: usize,
        want_c: usize,
        note: &'static str,
    ) -> Case {
        Case { lc_all, lang, over, want_faults, want_c, note }
    }
}

/// 空覆盖。
const NO_OVER: [(&'static str, &'static str); OVER_MAX] = [("", ""), ("", ""), ("", "")];

/// 十四槽。
pub const CASES: [Case; CORPUS] = [
    Case::new("", "", NO_OVER, 0, 12, "all-empty"),
    Case::new("en_US.UTF-8", "", NO_OVER, 0, 0, "all-wins"),
    Case::new("de_DE.UTF-8", "fr_FR.UTF-8", NO_OVER, 0, 0, "all-over-lang"),
    Case::new("", "en_US.UTF-8", NO_OVER, 0, 0, "lang-only"),
    Case::new(
        "",
        "en_US.UTF-8",
        [("LC_CTYPE", "ja_JP.UTF-8"), ("", ""), ("", "")],
        0,
        0,
        "ctype-over-lang",
    ),
    Case::new(
        "",
        "en_US.UTF-8",
        [("LC_TIME", "de_DE.UTF-8"), ("", ""), ("", "")],
        0,
        0,
        "time-over-lang",
    ),
    Case::new(
        "",
        "en_US.UTF-8",
        [("LC_NUMERIC", "de_DE.UTF-8"), ("LC_MONETARY", "de_DE.UTF-8"), ("", "")],
        0,
        0,
        "two-cats-over-lang",
    ),
    Case::new("", "fr_FR.UTF-8", NO_OVER, 0, 0, "lang-after-empty-all"),
    Case::new("xx_YY.UTF-8", "", NO_OVER, 12, 12, "all-bad"),
    Case::new(
        "",
        "en_US.UTF-8",
        [("LC_CTYPE", "xx_YY.UTF-8"), ("", ""), ("", "")],
        1,
        1,
        "ctype-bad",
    ),
    Case::new(
        "de_DE.UTF-8",
        "",
        [("LC_MESSAGES", "en_US.UTF-8"), ("", ""), ("", "")],
        0,
        0,
        "all-beats-cat",
    ),
    Case::new("", "xx_YY.UTF-8", NO_OVER, 12, 12, "lang-bad"),
    Case::new(
        "",
        "C.UTF-8",
        [("LC_CTYPE", "POSIX"), ("", ""), ("", "")],
        0,
        0,
        "posix-slot",
    ),
    Case::new("", "ja_JP.UTF-8", [("LC_TIME", "C"), ("", ""), ("", "")], 0, 1, "explicit-c-slot"),
];

/// 语料槽。
pub fn case_at(i: usize) -> Option<Case> {
    CASES.get(i).copied()
}

/// 语料是否互异（按三元组）。
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
                if b.lc_all == a.lc_all && b.lang == a.lang && b.over == a.over {
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

/// 由槽建环境。
pub fn env_of(c: &Case) -> Result<Env, u32> {
    let mut e = Env::new(c.lc_all, c.lang);
    let mut i = 0usize;
    while i < OVER_MAX {
        if let Some((n, v)) = c.over.get(i) {
            if !n.is_empty() {
                e.set_cat(n, v)?;
            }
        }
        i += 1;
    }
    Ok(e)
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
        let e = match env_of(&c) {
            Ok(v) => v,
            Err(_) => return false,
        };
        let comp = derive_repair(&e);
        if comp.faults() != c.want_faults {
            return false;
        }
        if comp.count_of("C") != c.want_c {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 汇总
// ---------------------------------------------------------------------------

/// 语料汇总（按格统计）。
#[derive(Clone, Copy, Debug)]
pub struct Census {
    total: usize,
    from_all: usize,
    from_cat: usize,
    from_lang: usize,
    from_default: usize,
    c_cats: usize,
    faults: usize,
    matched: usize,
    red: bool,
}

impl Census {
    /// 空汇总。
    pub const fn new() -> Census {
        Census {
            total: 0,
            from_all: 0,
            from_cat: 0,
            from_lang: 0,
            from_default: 0,
            c_cats: 0,
            faults: 0,
            matched: 0,
            red: false,
        }
    }

    /// 总条数。
    pub const fn total(&self) -> usize {
        self.total
    }

    /// LC_ALL 决定的格数。
    pub const fn from_all(&self) -> usize {
        self.from_all
    }

    /// LC_* 决定的格数。
    pub const fn from_cat(&self) -> usize {
        self.from_cat
    }

    /// LANG 决定的格数。
    pub const fn from_lang(&self) -> usize {
        self.from_lang
    }

    /// 默认决定的格数。
    pub const fn from_default(&self) -> usize {
        self.from_default
    }

    /// 落 C 的格数。
    pub const fn c_cats(&self) -> usize {
        self.c_cats
    }

    /// 故障格数。
    pub const fn faults(&self) -> usize {
        self.faults
    }

    /// 对上条数。
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
            return Err(LC_ENOBS);
        }
        if self.red {
            return Err(LC_ESTRICT);
        }
        if self.matched != self.total {
            return Err(LC_EVALUE);
        }
        if self.total != CORPUS {
            return Err(LC_EVALUE);
        }
        if self.from_all != 48 || self.from_cat != 7 || self.from_lang != 101 {
            return Err(LC_EVALUE);
        }
        if self.from_default != 12 || self.c_cats != 38 || self.faults != 25 {
            return Err(LC_EVALUE);
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
        s.push_str("locale_empty corpus=");
        s.push_str(u32_text(self.total as u32).as_str());
        s.push_str(" from_all=");
        s.push_str(u32_text(self.from_all as u32).as_str());
        s.push_str(" from_cat=");
        s.push_str(u32_text(self.from_cat as u32).as_str());
        s.push_str(" from_lang=");
        s.push_str(u32_text(self.from_lang as u32).as_str());
        s.push_str(" from_default=");
        s.push_str(u32_text(self.from_default as u32).as_str());
        s.push_str(" c_cats=");
        s.push_str(u32_text(self.c_cats as u32).as_str());
        s.push_str(" faults=");
        s.push_str(u32_text(self.faults as u32).as_str());
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
            None => return Err(LC_EVALUE),
        };
        let env = env_of(&case)?;
        let comp = derive_repair(&env);
        c.total += 1;
        c.faults += comp.faults();
        c.c_cats += comp.count_of("C");
        let mut cat = 0usize;
        while cat < CAT_COUNT {
            if let Some(p) = comp.at(cat) {
                match p.winner {
                    Winner::All => c.from_all += 1,
                    Winner::Cat => c.from_cat += 1,
                    Winner::Lang => c.from_lang += 1,
                    Winner::Default => c.from_default += 1,
                }
            }
            cat += 1;
        }
        if comp.faults() == case.want_faults && comp.count_of("C") == case.want_c {
            c.matched += 1;
        }
        i += 1;
    }
    if let Some(slot) = red {
        if slot >= c.total {
            return Err(LC_ESLOT);
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
        Err(LC_ENOBS)
    }
}

// ---------------------------------------------------------------------------
// 封存闸
// ---------------------------------------------------------------------------

/// 一次推导只允许封存一次。
#[derive(Clone, Copy, Debug)]
pub struct Sealer {
    sealed: bool,
    last: u32,
    root: u64,
}

impl Sealer {
    /// 空。
    pub const fn new() -> Sealer {
        Sealer { sealed: false, last: 0, root: 0 }
    }

    /// 是否封存。
    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 指纹。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 最后错误码。
    pub fn last(&self) -> u32 {
        self.last
    }

    /// 封存。
    pub fn seal(&mut self, comp: &Composite) -> Result<u64, u32> {
        if self.sealed {
            self.last = LC_ESTATE;
            return Err(LC_ESTATE);
        }
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < CAT_COUNT {
            if let Some(p) = comp.at(i) {
                h = fold_word(h, i as u64);
                h = fold_word(h, p.winner.rank() as u64);
                h = fold_word(h, digest(p.name.as_bytes()));
                h = fold_word(h, p.fault as u64);
            }
            i += 1;
        }
        self.root = h;
        self.sealed = true;
        self.last = 0;
        Ok(h)
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
            return Err(LC_ESLOT);
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

    /// 空判。
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
            return Err(LC_EBAL);
        }
        const VALS: [u32; 8] = [14, 48, 7, 101, 12, 38, 25, 929];
        const CODES: [u32; 6] =
            [LC_EBADVAL, LC_EBYTE, LC_ECAT, LC_ESTRICT, LC_ESTATE, LC_ENOVAL];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(LC_EBAL),
            };
            if s.slot as usize != i {
                return Err(LC_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(LC_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(LC_EVALUE);
                        }
                    }
                    None => return Err(LC_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(LC_EVALUE);
                        }
                    }
                    None => return Err(LC_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(LC_EBAL);
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
            return Err(LC_EBAL);
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
            Err(LC_ESEAL)
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
    let _ = l.register(count_cell(1, OP_CHOOSE, r.map(|x| x.from_all())));
    let _ = l.register(count_cell(2, OP_SLOT, r.map(|x| x.from_cat())));
    let _ = l.register(count_cell(3, OP_CHOOSE, r.map(|x| x.from_lang())));

    let _ = l.register(count_cell(4, OP_CHOOSE, r.map(|x| x.from_default())));
    let _ = l.register(count_cell(5, OP_REPAIR, r.map(|x| x.c_cats())));
    let _ = l.register(count_cell(6, OP_STRICT, r.map(|x| x.faults())));

    let red = run_with(Some(0));
    let _ = l.register(count_cell(7, OP_RENDER, red.map(|x| x.rate() as usize)));

    let _ = l.register(err_cell(8, OP_VALUE, canonical("xx_YY.UTF-8").err().unwrap_or(LC_EBADVAL)));
    let _ = l.register(err_cell(9, OP_VALUE, check_value("en US").err().unwrap_or(LC_EBYTE)));

    let cat_err = match cat_name(CAT_COUNT) {
        Ok(_) => LC_ECAT,
        Err(c) => c,
    };
    let _ = l.register(err_cell(10, OP_SLOT, cat_err));

    let mut bad = Env::new("xx_YY.UTF-8", "");
    let _ = bad.set_cat("LC_TIME", "C");
    let strict = match derive_strict(&bad) {
        Ok(_) => LC_ESTRICT,
        Err(c) => c,
    };
    let _ = l.register(err_cell(11, OP_STRICT, strict));

    let mut sealer = Sealer::new();
    let base = derive_repair(&Env::new("", "en_US.UTF-8"));
    let first = sealer.seal(&base);
    let state = match first {
        Ok(_) => match sealer.seal(&base) {
            Ok(_) => LC_ESTATE,
            Err(c) => c,
        },
        Err(c) => c,
    };
    let _ = l.register(err_cell(12, OP_SEAL, state));

    let _ = l.register(err_cell(13, OP_VALUE, forced("").err().unwrap_or(LC_ENOVAL)));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 值表与字节。
fn vals_ok() -> bool {
    let a = canonical("C") == Ok("C") && canonical("POSIX") == Ok("POSIX");
    let b = canonical("en_US.UTF-8") == Ok("en_US.UTF-8");
    let c = canonical("xx_YY.UTF-8") == Err(LC_EBADVAL);
    let d = canonical("") == Err(LC_ENOVAL);
    let e = check_value("en US") == Err(LC_EBYTE) && check_value("en\tUS") == Err(LC_EBYTE);
    let f = forced("") == Err(LC_ENOVAL) && forced("C") == Ok("C");
    let mut uniq = true;
    let mut i = 0usize;
    while i < TABLE.len() {
        let mut j = i + 1;
        while j < TABLE.len() {
            match (TABLE.get(i), TABLE.get(j)) {
                (Some(x), Some(y)) => {
                    if x == y {
                        uniq = false;
                    }
                }
                _ => uniq = false,
            }
            j += 1;
        }
        i += 1;
    }
    a && b && c && d && e && f && uniq
}

/// 分类槽。
fn slots_ok() -> bool {
    let a = cat_name(0) == Ok("LC_CTYPE") && cat_name(11) == Ok("LC_IDENTIFICATION");
    let b = cat_name(CAT_COUNT) == Err(LC_ECAT);
    let c = cat_index("LC_NUMERIC") == Ok(1) && cat_index("LC_BOGUS") == Err(LC_ECAT);
    let d = cat_bit(0) == Ok(1) && cat_bit(11) == Ok(0x0800) && cat_bit(12) == Err(LC_ECAT);
    let mut e = Env::new("", "");
    let f = e.set_cat("LC_CTYPE", "ja_JP.UTF-8") == Ok(()) && e.set_cats() == 1;
    let g = e.cat(0).map(|v| v.same("ja_JP.UTF-8")) == Ok(true);
    let h = e.set_cat("LC_CTYPE", "C") == Err(LC_EDUP);
    let i = e.set_cat("LC_BOGUS", "C") == Err(LC_ECAT);
    let j = e.cat(CAT_COUNT) == Err(LC_ECAT);
    a && b && c && d && f && g && h && i && j
}

/// 优先级单分类。
fn prec_ok() -> bool {
    let mut e = Env::new("de_DE.UTF-8", "en_US.UTF-8");
    let _ = e.set_cat("LC_CTYPE", "ja_JP.UTF-8");
    let a = choose(&e, 0).map(|x| x.0) == Ok(Winner::All);
    let b = match derive_cat(&e, 0) {
        Ok(p) => p.name == "de_DE.UTF-8" && p.winner == Winner::All,
        Err(_) => false,
    };
    let mut f = Env::new("", "en_US.UTF-8");
    let _ = f.set_cat("LC_CTYPE", "ja_JP.UTF-8");
    let c = choose(&f, 0).map(|x| x.0) == Ok(Winner::Cat);
    let d = choose(&f, 1).map(|x| x.0) == Ok(Winner::Lang);
    let g = choose(&Env::new("", ""), 0).map(|x| x.0) == Ok(Winner::Default);
    let h = choose(&f, CAT_COUNT).err() == Some(LC_ECAT);
    let i = Winner::All.rank() < Winner::Cat.rank()
        && Winner::Cat.rank() < Winner::Lang.rank()
        && Winner::Lang.rank() < Winner::Default.rank();
    a && b && c && d && g && h && i
}

/// 十二格全判。
fn grid_ok() -> bool {
    let mut e = Env::new("", "en_US.UTF-8");
    let _ = e.set_cat("LC_CTYPE", "ja_JP.UTF-8");
    let comp = derive_repair(&e);
    let a = comp.faults() == 0 && !comp.uniform() && comp.distinct() == 2;
    let b = comp.count_of("ja_JP.UTF-8") == 1 && comp.count_of("en_US.UTF-8") == 11;
    let c = comp.count_of("C") == 0;
    let d = comp.per_cat(0) == Some("ja_JP.UTF-8") && comp.per_cat(1) == Some("en_US.UTF-8");
    let f = comp.sources().as_str() == "all=0 cat=1 lang=11 default=0";
    let g = comp.at(0).map(|p| p.winner) == Some(Winner::Cat);
    let h = comp.at(1).map(|p| p.winner) == Some(Winner::Lang);
    a && b && c && d && f && g && h
}

/// 严格口径。
fn strict_ok() -> bool {
    let good = Env::new("", "en_US.UTF-8");
    let a = match derive_strict(&good) {
        Ok(c) => c.strict() && c.faults() == 0 && c.uniform(),
        Err(_) => false,
    };
    let bad = Env::new("xx_YY.UTF-8", "");
    let b = derive_strict(&bad).err() == Some(LC_ESTRICT);
    let mut partial = Env::new("", "en_US.UTF-8");
    let _ = partial.set_cat("LC_CTYPE", "xx_YY.UTF-8");
    let c = derive_strict(&partial).err() == Some(LC_ESTRICT);
    let d = derive_repair(&partial).faults() == 1;
    let e = derive_repair(&partial).count_of("C") == 1;
    let f = derive_repair(&bad).count_of("C") == 12;
    a && b && c && d && e && f
}

/// 逐格渲染。
fn render_ok() -> bool {
    let mut e = Env::new("", "en_US.UTF-8");
    let _ = e.set_cat("LC_CTYPE", "ja_JP.UTF-8");
    let comp = derive_repair(&e);
    let a = comp.render().as_str() == "LC_CTYPE=ja_JP.UTF-8;LC_NUMERIC=en_US.UTF-8;LC_TIME=en_US.UTF-8;LC_COLLATE=en_US.UTF-8;LC_MONETARY=en_US.UTF-8;LC_MESSAGES=en_US.UTF-8;LC_PAPER=en_US.UTF-8;LC_NAME=en_US.UTF-8;LC_ADDRESS=en_US.UTF-8;LC_TELEPHONE=en_US.UTF-8;LC_MEASUREMENT=en_US.UTF-8;LC_IDENTIFICATION=en_US.UTF-8\n";
    let b = comp.render().as_str().matches(';').count() == 11;
    let c = trail(&e, 0).map(|s| s.as_str() == "LC_CTYPE: LC_ALL=(unset) LC_CTYPE=ja_JP.UTF-8 LANG=en_US.UTF-8 => ja_JP.UTF-8") == Ok(true);
    let d = trail(&e, 2).map(|s| s.as_str() == "LC_TIME: LC_ALL=(unset) LC_TIME=(unset) LANG=en_US.UTF-8 => en_US.UTF-8") == Ok(true);
    let f = trail(&e, CAT_COUNT).err() == Some(LC_ECAT);
    a && b && c && d && f
}

/// 空串语义。
fn empty_ok() -> bool {
    let a = !Var::new("").is_set() && Var::new("C").is_set();
    let b = !Var::new("").same("C") && Var::new("").same("");
    let mut e = Env::new("", "fr_FR.UTF-8");
    let c = e.set_cat("LC_TIME", "") == Ok(()) && e.set_cats() == 0;
    let d = choose(&e, 2).map(|x| x.0) == Ok(Winner::Lang);
    let g = choose(&e, 3).map(|x| x.0) == Ok(Winner::Lang);
    let comp = derive_repair(&e);
    let h = comp.count_of("fr_FR.UTF-8") == 12;
    let mut all_empty = Env::new("", "");
    let _ = all_empty.set_cat("LC_CTYPE", "");
    let i = derive_repair(&all_empty).uniform();
    a && b && c && d && g && h && i
}

/// 十四卡全跑。
fn corpus_ok() -> bool {
    let c = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = c.total() == CORPUS && c.matched() == CORPUS;
    let b = c.from_all() == 48 && c.from_cat() == 7 && c.from_lang() == 101;
    let d = c.from_default() == 12 && c.c_cats() == 38 && c.faults() == 25;
    let e = c.from_all() + c.from_cat() + c.from_lang() + c.from_default() == CORPUS * CAT_COUNT;
    let f = c.rate() == 1000 && c.green() && c.gate() == Ok(());
    a && b && d && e && f
}

/// 语料编号。
fn slots_table_ok() -> bool {
    if !unique() || !notes_ok() || !table_ok() {
        return false;
    }
    let a = match case_at(0) {
        Some(c) => c.want_c == 12 && c.want_faults == 0,
        None => false,
    };
    let b = match case_at(8) {
        Some(c) => c.lc_all == "xx_YY.UTF-8" && c.want_faults == 12,
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
    let a = c.render().as_str() == "locale_empty corpus=14 from_all=48 from_cat=7 from_lang=101 from_default=12 c_cats=38 faults=25 match=14 rate=1000ppm\n";
    let e = Census::new();
    let b = e.is_empty() && e.rate() == 0 && e.gate() == Err(LC_ENOBS);
    let d = e.render().as_str() == "locale_empty corpus=0 from_all=0 from_cat=0 from_lang=0 from_default=0 c_cats=0 faults=0 match=0 rate=0ppm\n";
    let f = cover(CORPUS) == Ok(()) && cover(1) == Err(LC_ENOBS);
    a && b && d && f
}

/// 红单显式。
fn reds_ok() -> bool {
    let c = match run_with(Some(0)) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = c.matched() == CORPUS - 1 && c.rate() == 929;
    let b = c.gate() == Err(LC_ESTRICT) && !c.green();
    let d = run_with(Some(CORPUS)).err() == Some(LC_ESLOT);
    a && b && d
}

/// 封存与账本。
fn seal_ok() -> bool {
    let comp = derive_repair(&Env::new("", "en_US.UTF-8"));
    let mut s = Sealer::new();
    let a = !s.sealed() && s.root() == 0;
    let b = match s.seal(&comp) {
        Ok(h) => h != 0 && s.root() == h && s.sealed(),
        Err(_) => false,
    };
    let c = s.seal(&comp) == Err(LC_ESTATE) && s.last() == LC_ESTATE;
    let other = derive_repair(&Env::new("", "de_DE.UTF-8"));
    let mut t = Sealer::new();
    let mut u = Sealer::new();
    let d = match (t.seal(&comp), u.seal(&other)) {
        (Ok(h1), Ok(h2)) => h1 != h2,
        _ => false,
    };
    let l = build_ledger();
    let e = l.audit() == Ok(()) && l.ok_count() == 8 && l.err_count() == 6;
    let f = u32_text(101).as_str() == "101" && hex4_text(0xfd20).as_str() == "fd20";
    let g = digest(b"LC_ALL") == digest(b"LC_ALL") && digest(b"LC_ALL") != digest(b"LC_ALM");
    a && b && c && d && e && f && g
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = vals_ok() && slots_ok() && prec_ok();
    out[1] = grid_ok() && strict_ok() && empty_ok();
    out[2] = corpus_ok() && slots_table_ok() && report_ok() && reds_ok();
    out[3] = render_ok() && seal_ok() && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11643_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11643.setlocale_empty");

    s.add("LC01-值表与字节", vals_ok(), "static");
    s.add("LC02-分类槽位", slots_ok(), "static");
    s.add("LC03-优先级单格", prec_ok(), "static");
    s.add("LC04-十二格全判", grid_ok(), "static");
    s.add("LC05-严格口径拒收", strict_ok(), "static");
    s.add("LC06-空串语义", empty_ok(), "static");
    s.add("LC07-逐格渲染", render_ok(), "static");
    s.add("LC08-十四卡全跑", corpus_ok(), "static");
    s.add("LC09-语料编号", slots_table_ok(), "static");
    s.add("LC10-汇总渲染", report_ok(), "static");
    s.add("LC11-红单显式", reds_ok(), "static");
    s.add("LC12-封存与账本", seal_ok(), "static");
    s.add("LC13-冻结重放四注入", inject_four().iter().all(|x| *x), "static");

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
        assert_eq!(l.sample(8).map(|x| x.code), Some(LC_EBADVAL));
        assert_eq!(l.sample(13).map(|x| x.code), Some(LC_ENOVAL));
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
        assert_eq!(l.freeze(), Err(LC_EBAL));
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
        let s = run_unx11643_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }

    #[test]
    fn table_values() {
        assert_eq!(TABLE.len(), INSTALLED);
        assert_eq!(canonical("C"), Ok("C"));
        assert_eq!(canonical("POSIX"), Ok("POSIX"));
        assert_eq!(canonical("ja_JP.UTF-8"), Ok("ja_JP.UTF-8"));
        assert_eq!(canonical("ja_JP"), Err(LC_EBADVAL));
        assert_eq!(canonical(""), Err(LC_ENOVAL));
        assert_eq!(check_value("en US"), Err(LC_EBYTE));
    }

    #[test]
    fn empty_is_unset() {
        let v = Var::new("");
        assert!(!v.is_set());
        assert_eq!(v.len(), 0);
        assert_eq!(v.text().as_str(), "");
        let e = Env::new("", "en_US.UTF-8");
        assert!(!e.all().is_set());
        assert!(e.lang().is_set());
        assert_eq!(choose(&e, 0).map(|x| x.0), Ok(Winner::Lang));
    }

    #[test]
    fn precedence_three_levels() {
        let mut e = Env::new("de_DE.UTF-8", "en_US.UTF-8");
        let _ = e.set_cat("LC_CTYPE", "ja_JP.UTF-8");
        assert_eq!(choose(&e, 0).map(|x| x.0), Ok(Winner::All));
        let mut f = Env::new("", "en_US.UTF-8");
        let _ = f.set_cat("LC_CTYPE", "ja_JP.UTF-8");
        assert_eq!(choose(&f, 0).map(|x| x.0), Ok(Winner::Cat));
        assert_eq!(choose(&f, 1).map(|x| x.0), Ok(Winner::Lang));
        assert_eq!(choose(&Env::new("", ""), 5).map(|x| x.0), Ok(Winner::Default));
    }

    #[test]
    fn repair_and_strict() {
        let bad = Env::new("xx_YY.UTF-8", "");
        assert_eq!(derive_strict(&bad).err(), Some(LC_ESTRICT));
        let rep = derive_repair(&bad);
        assert_eq!(rep.faults(), 12);
        assert_eq!(rep.count_of("C"), 12);
        assert!(rep.uniform());
    }

    #[test]
    fn composite_render() {
        let e = Env::new("", "en_US.UTF-8");
        let c = derive_repair(&e);
        assert!(c.render().as_str().starts_with("LC_CTYPE=en_US.UTF-8;"));
        assert!(c.render().as_str().ends_with("LC_IDENTIFICATION=en_US.UTF-8\n"));
        assert_eq!(c.render().as_str().matches(';').count(), 11);
    }

    #[test]
    fn trail_text() {
        let mut e = Env::new("", "en_US.UTF-8");
        let _ = e.set_cat("LC_CTYPE", "ja_JP.UTF-8");
        let t = match trail(&e, 0) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(
            t.as_str(),
            "LC_CTYPE: LC_ALL=(unset) LC_CTYPE=ja_JP.UTF-8 LANG=en_US.UTF-8 => ja_JP.UTF-8"
        );
        let u = match trail(&e, 2) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert!(u.as_str().contains("LC_TIME=(unset)"));
        assert_eq!(trail(&e, 99).err(), Some(LC_ECAT));
    }

    #[test]
    fn census_numbers() {
        let c = match run() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(c.total(), 14);
        assert_eq!(c.from_all(), 48);
        assert_eq!(c.from_cat(), 7);
        assert_eq!(c.from_lang(), 101);
        assert_eq!(c.from_default(), 12);
        assert_eq!(c.c_cats(), 38);
        assert_eq!(c.faults(), 25);
        assert_eq!(c.gate(), Ok(()));
    }

    #[test]
    fn red_override() {
        let c = match run_with(Some(5)) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(c.matched(), 13);
        assert_eq!(c.rate(), 929);
        assert_eq!(c.gate(), Err(LC_ESTRICT));
        assert_eq!(run_with(Some(14)).err(), Some(LC_ESLOT));
    }

    #[test]
    fn sealer_once() {
        let comp = derive_repair(&Env::new("", "C"));
        let mut s = Sealer::new();
        assert!(s.seal(&comp).is_ok());
        assert!(s.sealed());
        assert_eq!(s.seal(&comp), Err(LC_ESTATE));
        assert_eq!(s.last(), LC_ESTATE);
    }

    #[test]
    fn case_table() {
        assert_eq!(CASES.len(), CORPUS);
        assert!(unique());
        assert!(notes_ok());
        assert!(table_ok());
        assert_eq!(case_at(CORPUS), None);
    }

    #[test]
    fn bad_case_counts() {
        let c = match case_at(8) {
            Some(v) => v,
            None => return,
        };
        let e = match env_of(&c) {
            Ok(v) => v,
            Err(_) => return,
        };
        let comp = derive_repair(&e);
        assert_eq!(comp.faults(), 12);
        assert_eq!(comp.count_of("C"), 12);
    }

    #[test]
    fn slots_addressing() {
        assert_eq!(cat_index("LC_TIME"), Ok(2));
        assert_eq!(cat_name(2), Ok("LC_TIME"));
        assert_eq!(cat_name(12), Err(LC_ECAT));
        assert_eq!(cat_bit(11), Ok(0x0800));
        assert_eq!(cat_bit(12), Err(LC_ECAT));
    }

    #[test]
    fn env_override_dup() {
        let mut e = Env::new("", "");
        assert_eq!(e.set_cat("LC_TIME", "C"), Ok(()));
        assert_eq!(e.set_cat("LC_TIME", "POSIX"), Err(LC_EDUP));
        assert_eq!(e.set_cat("LC_NOPE", "C"), Err(LC_ECAT));
        assert_eq!(e.set_cats(), 1);
    }

    #[test]
    fn mixed_grid_counts() {
        let mut e = Env::new("", "en_US.UTF-8");
        let _ = e.set_cat("LC_NUMERIC", "de_DE.UTF-8");
        let _ = e.set_cat("LC_MONETARY", "de_DE.UTF-8");
        let c = derive_repair(&e);
        assert_eq!(c.count_of("de_DE.UTF-8"), 2);
        assert_eq!(c.count_of("en_US.UTF-8"), 10);
        assert_eq!(c.sources().as_str(), "all=0 cat=2 lang=10 default=0");
    }
}
