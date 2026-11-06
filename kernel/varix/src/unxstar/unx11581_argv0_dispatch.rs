//! UNX-F11581 工具箱 argv[0] 派发判据（基名 → applet 映射，F11204 同序）
//!
//! busybox 的多调用二进制把「我是谁」完全交给 argv[0]：内核 `execve` 只递进
//! 一个路径串，busybox 取**基名**（最后一个 `/` 之后的部分）当 applet 名，
//! 用一次字符串查表决定走哪张命令面。这条判定看着只有一行，却是整块工具箱
//! 的总开关——装错的软链、带尾斜杠的调用、把 `busybox ls` 的首参当成路径的
//! 误判，全都从这里漏进来。本模块把该判定拆成九道可证伪的闸门：
//!
//! ```text
//! 空 argv        → 连「我是谁」都没有，拒
//! 非打印字节      → argv[0] 里带控制字符，拒（execve 不会给，但注入会）
//! 路径超长        → 超过 PATH_CAP，拒（防止后续拼接越界）
//! 尾斜杠 / 点号   → `bin/`、`.`、`..` 不是可执行名，拒
//! 基名查表        → 40 条自举表逐字节比对，匿名即「未知 applet」
//! 形态            → 基名 busybox 时第二参才是 applet（多调），否则直呼
//! 全局选项        → busybox 的第二参以 `-` 开头时属于 busybox 自己（--list/--help）
//! 命中记账        → 每次派发落一格计数，用于热度与去重统计
//! ```
//!
//! 闸门次序：空参 → 字节 → 长度 → 尾斜杠 → 点号 → 基名 → 表查找 → 形态 → 记账。
//!
//! 与 F11204 的关系：F11204 立的是「40 applet 自举 + 管道段」的总协议，本卡
//! 只钉 **argv[0] 基名到 applet 序号** 这一段判定，并把「子表过滤」（只放行
//! 一部分 applet 的精简 busybox）也纳入同一套错误码。表序与 F11204 的
//! `NAMES` 常量逐条对齐，任何一条漂移都会被 `AD04` 判据当场打红。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本 14 格
//! （8 成 6 败）在 `audit` 逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf960;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// applet 总数（与 F11204 自举序一致）。
pub const APPLETS: usize = 40;

/// 基名容量。
pub const NAME_CAP: usize = 32;

/// 整条 argv[0] 路径容量。
pub const PATH_CAP: usize = 96;

/// 多调二进制名。
pub const BUSYBOX: &str = "busybox";

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 空 argv 或缺 applet 参数。
pub const AD_EEMPTY: u32 = SEG_BASE + 0x01;
/// 非打印字节。
pub const AD_EBYTE: u32 = SEG_BASE + 0x02;
/// 超长。
pub const AD_ELEN: u32 = SEG_BASE + 0x03;
/// 尾斜杠（基名为空）。
pub const AD_ETRAIL: u32 = SEG_BASE + 0x04;
/// 点号基名（`.` / `..`）。
pub const AD_EDOT: u32 = SEG_BASE + 0x05;
/// 表内重名。
pub const AD_EDUP: u32 = SEG_BASE + 0x06;
/// 未知 applet。
pub const AD_EUNKNOWN: u32 = SEG_BASE + 0x07;
/// 选项非法。
pub const AD_EOPT: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const AD_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const AD_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const AD_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const AD_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const AD_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// argv[0] 拆解。
pub const OP_PARSE: u8 = 0x01;
/// 表查找。
pub const OP_LOOKUP: u8 = 0x02;
/// 形态判定。
pub const OP_RESOLVE: u8 = 0x03;
/// 命中记账。
pub const OP_HIT: u8 = 0x04;
/// 子表装配。
pub const OP_TABLE: u8 = 0x05;
/// 族别统计。
pub const OP_FAMILY: u8 = 0x06;

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

/// 是否可打印（0x21..=0x7e）。
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

/// 拼接字节为 String。
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
// 族别与自举表
// ---------------------------------------------------------------------------

/// applet 族别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// 壳。
    Shell,
    /// 文件。
    File,
    /// 文本。
    Text,
    /// 进程与挂载。
    Proc,
    /// 杂项。
    Misc,
}

impl Family {
    /// 族别代号。
    pub fn code(&self) -> u8 {
        match self {
            Family::Shell => 0,
            Family::File => 1,
            Family::Text => 2,
            Family::Proc => 3,
            Family::Misc => 4,
        }
    }

    /// 族别文本。
    pub fn text(&self) -> &'static str {
        match self {
            Family::Shell => "shell",
            Family::File => "file",
            Family::Text => "text",
            Family::Proc => "proc",
            Family::Misc => "misc",
        }
    }
}

/// 全部 applet 名（与 F11204 自举序一致，40 条）。
pub const APPLET_NAMES: [&str; APPLETS] = [
    "sh", "ash", "cat", "cp", "mv", "rm", "ls", "ln", "mkdir", "rmdir",
    "chmod", "chown", "touch", "stat", "df", "du", "find", "grep", "sed", "awk",
    "cut", "sort", "uniq", "wc", "head", "tail", "tr", "tee", "echo", "printf",
    "test", "expr", "mount", "umount", "ps", "kill", "sleep", "date", "dd", "tar",
];

/// 族别表（与 `APPLET_NAMES` 同序、同长）。
const FAMILY_TABLE: [u8; APPLETS] = [
    0, 0, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 2, 2, 2, 2,
    2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
    4, 4, 3, 3, 3, 3, 3, 3, 1, 1,
];

/// 查表结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppletRef {
    /// 序号（表下标）。
    pub id: u8,
    /// 名。
    pub name: &'static str,
    /// 族别。
    pub family: Family,
}

impl AppletRef {
    /// 单行文本：`#6 ls file`。
    pub fn text(&self) -> String {
        let mut s = String::new();
        s.push('#');
        push_u64(&mut s, self.id as u64);
        s.push(' ');
        s.push_str(self.name);
        s.push(' ');
        s.push_str(self.family.text());
        s
    }
}

/// 由族别代号取枚举。
pub fn family_of(code: u8) -> Option<Family> {
    match code {
        0 => Some(Family::Shell),
        1 => Some(Family::File),
        2 => Some(Family::Text),
        3 => Some(Family::Proc),
        4 => Some(Family::Misc),
        _ => None,
    }
}

/// 表查找：逐字节比对，大小写敏感。
pub fn lookup(name: &[u8]) -> Option<AppletRef> {
    let mut i = 0usize;
    while i < APPLETS {
        if let Some(t) = APPLET_NAMES.get(i) {
            if eq_bytes(t.as_bytes(), name) {
                let id = i as u8;
                let f = match FAMILY_TABLE.get(i) {
                    Some(c) => family_of(*c)?,
                    None => return None,
                };
                return Some(AppletRef { id, name: t, family: f });
            }
        }
        i += 1;
    }
    None
}

/// 按族别计数。
pub fn count_family(f: Family) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < APPLETS {
        if let Some(c) = FAMILY_TABLE.get(i) {
            if *c == f.code() {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 名表里是否存在（不取族别）。
pub fn known(name: &[u8]) -> bool {
    lookup(name).is_some()
}

// ---------------------------------------------------------------------------
// argv[0] 拆解
// ---------------------------------------------------------------------------

/// 拆好的 argv[0]：目录段 + 基名段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Arg0 {
    dir: [u8; PATH_CAP],
    dir_len: usize,
    base: [u8; NAME_CAP],
    base_len: usize,
}

impl Arg0 {
    /// 空白。
    pub const fn blank() -> Arg0 {
        Arg0 { dir: [0u8; PATH_CAP], dir_len: 0, base: [0u8; NAME_CAP], base_len: 0 }
    }

    /// 拆解：空 → `EEMPTY`，不可打印 → `EBYTE`，超长 → `ELEN`，
    /// 尾斜杠 → `ETRAIL`，`.`/`..` → `EDOT`。
    pub fn new(s: &[u8]) -> Result<Arg0, u32> {
        if s.is_empty() {
            return Err(AD_EEMPTY);
        }
        if s.len() > PATH_CAP {
            return Err(AD_ELEN);
        }
        if !printable(s) {
            return Err(AD_EBYTE);
        }
        let mut last: Option<usize> = None;
        let mut i = 0usize;
        while i < s.len() {
            if let Some(b) = s.get(i) {
                if *b == b'/' {
                    last = Some(i);
                }
            }
            i += 1;
        }
        let dir_len = match last {
            Some(p) => p,
            None => 0,
        };
        let start = match last {
            Some(p) => p.saturating_add(1),
            None => 0,
        };
        if start >= s.len() {
            return Err(AD_ETRAIL);
        }
        let base_len = s.len().saturating_sub(start);
        if base_len > NAME_CAP {
            return Err(AD_ELEN);
        }
        if let Some(b) = s.get(start) {
            if *b == b'.' {
                let only_dot = base_len == 1;
                let only_dotdot = base_len == 2 && s.get(start.saturating_add(1)) == Some(&b'.');
                if only_dot || only_dotdot {
                    return Err(AD_EDOT);
                }
            }
        }
        let mut a = Arg0::blank();
        let mut k = 0usize;
        while k < dir_len {
            match (s.get(k), a.dir.get_mut(k)) {
                (Some(src), Some(dst)) => *dst = *src,
                _ => return Err(AD_ELEN),
            }
            k += 1;
        }
        a.dir_len = dir_len;
        let mut j = 0usize;
        while j < base_len {
            match (s.get(start.saturating_add(j)), a.base.get_mut(j)) {
                (Some(src), Some(dst)) => *dst = *src,
                _ => return Err(AD_ELEN),
            }
            j += 1;
        }
        a.base_len = base_len;
        Ok(a)
    }

    /// 目录段字节。
    pub fn dir(&self) -> &[u8] {
        self.dir.get(0..self.dir_len).unwrap_or(&[])
    }

    /// 基名段字节。
    pub fn base(&self) -> &[u8] {
        self.base.get(0..self.base_len).unwrap_or(&[])
    }

    /// 目录段长度（不含尾斜杠）。
    pub fn dir_len(&self) -> usize {
        self.dir_len
    }

    /// 基名长度。
    pub fn base_len(&self) -> usize {
        self.base_len
    }

    /// 目录段文本。
    pub fn dir_text(&self) -> String {
        text_of(self.dir())
    }

    /// 基名文本。
    pub fn base_text(&self) -> String {
        text_of(self.base())
    }

    /// 是否多调二进制本体（基名恰为 `busybox`）。
    pub fn is_busybox(&self) -> bool {
        eq_bytes(self.base(), BUSYBOX.as_bytes())
    }
}

// ---------------------------------------------------------------------------
// 形态判定
// ---------------------------------------------------------------------------

/// 派发来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// 直接以 applet 名调用（软链或改名）。
    Direct,
    /// 以 busybox 为首参、applet 为第二参。
    Multi,
    /// 落在 busybox 自身的全局选项上。
    Global,
}

impl Origin {
    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            Origin::Direct => "direct",
            Origin::Multi => "multi",
            Origin::Global => "global",
        }
    }
}

/// 一次成功派发。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    /// applet 序号。
    pub id: u8,
    /// 来源形态。
    pub origin: Origin,
    /// 被吃掉的 argv 前缀个数（直呼 1，多调 2）。
    pub skip: usize,
    /// 余下实参个数。
    pub argc: usize,
}

/// 派发结果：applet 或 busybox 自己的全局动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// 落到某个 applet。
    Applet(Hit),
    /// 落到全局动作（`list` / `help`）。
    Global(&'static str),
}

impl Route {
    /// 是否 applet。
    pub fn applet_id(&self) -> Option<u8> {
        match self {
            Route::Applet(h) => Some(h.id),
            Route::Global(_) => None,
        }
    }

    /// 全局动作名。
    pub fn global_name(&self) -> Option<&'static str> {
        match self {
            Route::Applet(_) => None,
            Route::Global(n) => Some(n),
        }
    }
}

/// busybox 第二参的全局动作。
fn global_of(a1: &[u8]) -> Result<Route, u32> {
    if eq_bytes(a1, b"--list") {
        return Ok(Route::Global("list"));
    }
    if eq_bytes(a1, b"--help") || eq_bytes(a1, b"-h") {
        return Ok(Route::Global("help"));
    }
    if a1.len() <= 1 {
        return Err(AD_EBYTE);
    }
    Err(AD_EOPT)
}

/// 按标准 40 表派发。
pub fn resolve(argv: &[&[u8]]) -> Result<Route, u32> {
    let a0 = match argv.get(0) {
        Some(v) => *v,
        None => return Err(AD_EEMPTY),
    };
    let p = Arg0::new(a0)?;
    if p.is_busybox() {
        let a1 = match argv.get(1) {
            Some(v) => *v,
            None => return Err(AD_EEMPTY),
        };
        if a1.is_empty() {
            return Err(AD_EEMPTY);
        }
        if let Some(f) = a1.get(0) {
            if *f == b'-' {
                return global_of(a1);
            }
        }
        if !printable(a1) {
            return Err(AD_EBYTE);
        }
        if a1.len() > NAME_CAP {
            return Err(AD_ELEN);
        }
        let r = match lookup(a1) {
            Some(v) => v,
            None => return Err(AD_EUNKNOWN),
        };
        return Ok(Route::Applet(Hit {
            id: r.id,
            origin: Origin::Multi,
            skip: 2,
            argc: argv.len().saturating_sub(2),
        }));
    }
    let r = match lookup(p.base()) {
        Some(v) => v,
        None => return Err(AD_EUNKNOWN),
    };
    Ok(Route::Applet(Hit {
        id: r.id,
        origin: Origin::Direct,
        skip: 1,
        argc: argv.len().saturating_sub(1),
    }))
}

// ---------------------------------------------------------------------------
// 子表（精简 busybox）
// ---------------------------------------------------------------------------

/// 只放行一部分 applet 的子表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Table {
    ids: [u8; APPLETS],
    len: usize,
}

impl Table {
    /// 空表。
    pub const fn new() -> Table {
        Table { ids: [0u8; APPLETS], len: 0 }
    }

    /// 由名表装配：未知名 → `EUNKNOWN`，重名 → `EDUP`，超容 → `ELEN`。
    pub fn from_names(names: &[&[u8]]) -> Result<Table, u32> {
        if names.len() > APPLETS {
            return Err(AD_ELEN);
        }
        let mut t = Table::new();
        let mut i = 0usize;
        while i < names.len() {
            let n = match names.get(i) {
                Some(v) => *v,
                None => return Err(AD_EUNKNOWN),
            };
            let r = match lookup(n) {
                Some(v) => v,
                None => return Err(AD_EUNKNOWN),
            };
            if t.contains(r.id) {
                return Err(AD_EDUP);
            }
            if let Some(dst) = t.ids.get_mut(t.len) {
                *dst = r.id;
            }
            t.len += 1;
            i += 1;
        }
        Ok(t)
    }

    /// 标准全表。
    pub fn full() -> Result<Table, u32> {
        let mut t = Table::new();
        let mut i = 0usize;
        while i < APPLETS {
            if let Some(dst) = t.ids.get_mut(i) {
                *dst = i as u8;
            }
            i += 1;
        }
        t.len = APPLETS;
        Ok(t)
    }

    /// 项数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 是否含某序号。
    pub fn contains(&self, id: u8) -> bool {
        let mut i = 0usize;
        while i < self.len {
            if self.ids.get(i) == Some(&id) {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 子表内查找。
    pub fn find(&self, name: &[u8]) -> Option<u8> {
        let r = lookup(name)?;
        if self.contains(r.id) {
            Some(r.id)
        } else {
            None
        }
    }

    /// 第 `i` 项。
    pub fn at(&self, i: usize) -> Option<AppletRef> {
        if i >= self.len {
            return None;
        }
        let id = *self.ids.get(i)?;
        APPLET_NAMES.get(id as usize).map(|n| AppletRef {
            id,
            name: n,
            family: match FAMILY_TABLE.get(id as usize) {
                Some(c) => match family_of(*c) {
                    Some(f) => f,
                    None => Family::Misc,
                },
                None => Family::Misc,
            },
        })
    }

    /// 名字空格拼接。
    pub fn names_text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.at(i) {
                if !s.is_empty() {
                    s.push(' ');
                }
                s.push_str(r.name);
            }
            i += 1;
        }
        s
    }

    /// 子表内派发（形态同上，但未知/被裁掉一律 `EUNKNOWN`）。
    pub fn resolve(&self, argv: &[&[u8]]) -> Result<Route, u32> {
        let a0 = match argv.get(0) {
            Some(v) => *v,
            None => return Err(AD_EEMPTY),
        };
        let p = Arg0::new(a0)?;
        if p.is_busybox() {
            let a1 = match argv.get(1) {
                Some(v) => *v,
                None => return Err(AD_EEMPTY),
            };
            if a1.is_empty() {
                return Err(AD_EEMPTY);
            }
            if let Some(f) = a1.get(0) {
                if *f == b'-' {
                    return global_of(a1);
                }
            }
            let id = match self.find(a1) {
                Some(v) => v,
                None => return Err(AD_EUNKNOWN),
            };
            return Ok(Route::Applet(Hit {
                id,
                origin: Origin::Multi,
                skip: 2,
                argc: argv.len().saturating_sub(2),
            }));
        }
        let id = match self.find(p.base()) {
            Some(v) => v,
            None => return Err(AD_EUNKNOWN),
        };
        Ok(Route::Applet(Hit {
            id,
            origin: Origin::Direct,
            skip: 1,
            argc: argv.len().saturating_sub(1),
        }))
    }
}

// ---------------------------------------------------------------------------
// 命中账
// ---------------------------------------------------------------------------

/// 派发命中计数板。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hits {
    counts: [u32; APPLETS],
    total: u32,
}

impl Hits {
    /// 空板。
    pub const fn new() -> Hits {
        Hits { counts: [0u32; APPLETS], total: 0 }
    }

    /// 记一次命中。
    pub fn note(&mut self, id: u8) -> Result<(), u32> {
        if (id as usize) >= APPLETS {
            return Err(AD_EUNKNOWN);
        }
        if let Some(c) = self.counts.get_mut(id as usize) {
            *c = c.saturating_add(1);
        }
        self.total = self.total.saturating_add(1);
        Ok(())
    }

    /// 记一次派发结果（全局动作不计数）。
    pub fn note_route(&mut self, r: &Route) -> Result<(), u32> {
        match r {
            Route::Applet(h) => self.note(h.id),
            Route::Global(_) => Ok(()),
        }
    }

    /// 某 applet 计数。
    pub fn get(&self, id: u8) -> u32 {
        self.counts.get(id as usize).copied().unwrap_or(0)
    }

    /// 总命中数。
    pub fn total(&self) -> u32 {
        self.total
    }

    /// 不同 applet 数。
    pub fn distinct(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < APPLETS {
            if let Some(c) = self.counts.get(i) {
                if *c > 0 {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 最热 applet（并列取序号小者）；空板 → None。
    pub fn top(&self) -> Option<u8> {
        if self.total == 0 {
            return None;
        }
        let mut best: Option<u8> = None;
        let mut max = 0u32;
        let mut i = 0usize;
        while i < APPLETS {
            if let Some(c) = self.counts.get(i) {
                if *c > max {
                    max = *c;
                    best = Some(i as u8);
                }
            }
            i += 1;
        }
        best
    }

    /// 报表：`#6=2 #17=1`。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < APPLETS {
            if let Some(c) = self.counts.get(i) {
                if *c > 0 {
                    if !s.is_empty() {
                        s.push(' ');
                    }
                    s.push('#');
                    push_u64(&mut s, i as u64);
                    s.push('=');
                    push_u64(&mut s, *c as u64);
                }
            }
            i += 1;
        }
        s
    }
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

    /// 登记一格（槽位必须等于当前长度）。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(AD_ESLOT);
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
        self.len.saturating_sub(self.ok_count())
    }

    /// 对账：14 格、8 成 6 败、成功值序与失败码序逐格相符。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(AD_EBAL);
        }
        const VALS: [u32; 8] = [2, 8, 17, 40, 16, 6, 2, 3];
        const CODES: [u32; 6] = [
            AD_ETRAIL,
            AD_EDOT,
            AD_EBYTE,
            AD_EDUP,
            AD_EUNKNOWN,
            AD_EOPT,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(AD_EBAL),
            };
            if s.slot as usize != i {
                return Err(AD_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(AD_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(AD_EVALUE);
                        }
                    }
                    None => return Err(AD_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(AD_EVALUE);
                        }
                    }
                    None => return Err(AD_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(AD_EBAL);
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
            return Err(AD_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放：重新组装一本并与自身指纹比对。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let a = fresh.seal();
        if a == self.seal() {
            Ok(a)
        } else {
            Err(AD_ESEAL)
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
// 样例
// ---------------------------------------------------------------------------

const N_LS: &[u8] = b"ls";
const N_GREP: &[u8] = b"grep";
const N_CAT: &[u8] = b"cat";
const N_BUSYBOX: &[u8] = b"busybox";
const N_BIN_LS: &[u8] = b"/bin/ls";
const N_BIN_BUSYBOX: &[u8] = b"/usr/bin/busybox";
const N_MULTI_LS: &[u8] = b"/bin/busybox ls";
const N_TAIL_SLASH: &[u8] = b"bin/";
const N_DOT: &[u8] = b".";
const N_DOTDOT: &[u8] = b"..";

/// `Result<u32, u32>` → 样本（成功取值、失败取码）。
fn cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 派发成功取 applet 序号，其它一律记 `ENOBS`（不进账本）。
fn routed_id(r: Result<Route, u32>) -> Result<u32, u32> {
    match r {
        Ok(Route::Applet(h)) => Ok(h.id as u32),
        Ok(Route::Global(_)) => Err(AD_ENOBS),
        Err(c) => Err(c),
    }
}

/// 账本组装：14 格，8 成 6 败，顺序固定。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(cell(
        0,
        OP_PARSE,
        Arg0::new(N_BIN_LS).map(|a| a.base_len() as u32),
    ));

    let _ = l.register(cell(
        1,
        OP_PARSE,
        Arg0::new(N_BIN_BUSYBOX).map(|a| a.dir_len() as u32),
    ));

    let _ = l.register(cell(
        2,
        OP_LOOKUP,
        lookup(N_GREP).map(|r| r.id as u32).ok_or(AD_EUNKNOWN),
    ));

    let _ = l.register(cell(
        3,
        OP_TABLE,
        Table::full().map(|t| t.len() as u32),
    ));

    let _ = l.register(cell(4, OP_FAMILY, Ok(count_family(Family::File) as u32)));

    let _ = l.register(cell(5, OP_RESOLVE, routed_id(resolve(&[N_BIN_LS]))));

    let _ = l.register(cell(
        6,
        OP_RESOLVE,
        match resolve(&[N_BIN_BUSYBOX, N_LS, b"-l", b"/tmp"]) {
            Ok(Route::Applet(h)) => Ok(h.skip as u32),
            _ => Err(AD_ENOBS),
        },
    ));

    let _ = l.register(cell(7, OP_HIT, hit_distinct()));

    let _ = l.register(cell(
        8,
        OP_PARSE,
        Arg0::new(N_TAIL_SLASH).map(|a| a.base_len() as u32),
    ));

    let _ = l.register(cell(9, OP_PARSE, Arg0::new(N_DOT).map(|a| a.base_len() as u32)));

    let _ = l.register(cell(
        10,
        OP_PARSE,
        Arg0::new(&[0x01u8]).map(|a| a.base_len() as u32),
    ));

    let _ = l.register(cell(
        11,
        OP_TABLE,
        Table::from_names(&[N_LS, N_LS]).map(|t| t.len() as u32),
    ));

    let _ = l.register(cell(
        12,
        OP_RESOLVE,
        routed_id(resolve(&[b"/opt/bin/gerp"])),
    ));

    let _ = l.register(cell(
        13,
        OP_RESOLVE,
        routed_id(resolve(&[N_BUSYBOX, b"--bogus"])),
    ));

    l
}

/// 三格命中后的去重数（`ls` 两次、`grep`、`cat` 各一次）。
fn hit_distinct() -> Result<u32, u32> {
    let mut h = Hits::new();
    h.note(6)?;
    h.note(17)?;
    h.note(6)?;
    h.note(2)?;
    Ok(h.distinct() as u32)
}

// ---------------------------------------------------------------------------
// 判据面
// ---------------------------------------------------------------------------

/// 基名拆解面。
fn basename_ok() -> bool {
    let a = match Arg0::new(N_BIN_LS) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if !eq_bytes(a.dir(), b"/bin") || !eq_bytes(a.base(), b"ls") {
        return false;
    }
    if a.dir_len() != 4 || a.base_len() != 2 {
        return false;
    }
    let b = match Arg0::new(b"ls") {
        Ok(v) => v,
        Err(_) => return false,
    };
    if b.dir_len() != 0 || b.is_busybox() {
        return false;
    }
    let c = match Arg0::new(b"./ls") {
        Ok(v) => v,
        Err(_) => return false,
    };
    if !eq_bytes(c.dir(), b".") || c.base_text().as_str() != "ls" {
        return false;
    }
    let d = match Arg0::new(N_BIN_BUSYBOX) {
        Ok(v) => v,
        Err(_) => return false,
    };
    d.is_busybox() && d.dir_text().as_str() == "/usr/bin" && d.base_text().as_str() == "busybox"
}

/// 字节与长度面。
fn byte_ok() -> bool {
    let a = Arg0::new(b"").err() == Some(AD_EEMPTY);
    let b = Arg0::new(&[0x7f]).err() == Some(AD_EBYTE);
    let c = Arg0::new(&[b'a'; 97]).err() == Some(AD_ELEN);
    let d = Arg0::new(&[b'a'; 32]).map(|v| v.base_len()) == Ok(32);
    let e = Arg0::new(&[b'a'; 33]).err() == Some(AD_ELEN);
    let f = Arg0::new(b"a b").err() == Some(AD_EBYTE);
    a && b && c && d && e && f
}

/// 尾斜杠与点号面。
fn tail_ok() -> bool {
    let a = Arg0::new(N_TAIL_SLASH).err() == Some(AD_ETRAIL);
    let b = Arg0::new(b"/").err() == Some(AD_ETRAIL);
    let c = Arg0::new(N_DOT).err() == Some(AD_EDOT);
    let d = Arg0::new(N_DOTDOT).err() == Some(AD_EDOT);
    let e = Arg0::new(b"/bin/..").err() == Some(AD_EDOT);
    let f = Arg0::new(b".ls").map(|v| v.base_len()) == Ok(3);
    let g = Arg0::new(b"...").map(|v| v.base_len()) == Ok(3);
    a && b && c && d && e && f && g
}

/// 四十表查找面。
fn lookup_ok() -> bool {
    let mut i = 0usize;
    while i < APPLETS {
        let n = match APPLET_NAMES.get(i) {
            Some(v) => v.as_bytes(),
            None => return false,
        };
        let r = match lookup(n) {
            Some(v) => v,
            None => return false,
        };
        if r.id as usize != i || r.name.as_bytes() != n {
            return false;
        }
        i += 1;
    }
    if lookup(b"LS").is_some() || lookup(b"lss").is_some() || lookup(b"true").is_some() {
        return false;
    }
    if lookup(b"sh ").is_some() || lookup(b"").is_some() {
        return false;
    }
    known(b"tar") && !known(b"tarx")
}

/// 族别面。
fn family_ok() -> bool {
    let a = count_family(Family::Shell) == 2;
    let b = count_family(Family::File) == 16;
    let c = count_family(Family::Text) == 14;
    let d = count_family(Family::Proc) == 6;
    let e = count_family(Family::Misc) == 2;
    let f = Family::File.code() == 1 && Family::Text.text() == "text";
    let g = lookup(b"ls").map(|r| r.family) == Some(Family::File);
    let h = lookup(b"sh").map(|r| r.family) == Some(Family::Shell);
    let i = lookup(b"mount").map(|r| r.family) == Some(Family::Proc);
    let j = lookup(b"expr").map(|r| r.family) == Some(Family::Misc);
    let k = family_of(5).is_none() && family_of(0) == Some(Family::Shell);
    let l = lookup(b"ls").map(|r| r.text().as_str() == "#6 ls file") == Some(true);
    a && b && c && d && e && f && g && h && i && j && k && l
}

/// 直呼形态面。
fn direct_ok() -> bool {
    let a = match resolve(&[N_BIN_LS, b"-l", b"/tmp"]) {
        Ok(Route::Applet(h)) => h.id == 6 && h.origin == Origin::Direct && h.skip == 1 && h.argc == 2,
        _ => false,
    };
    let b = match resolve(&[N_LS]) {
        Ok(Route::Applet(h)) => h.id == 6 && h.argc == 0 && h.skip == 1,
        _ => false,
    };
    let c = match resolve(&[b"/usr/local/bin/tar", b"xf", b"a.tar"]) {
        Ok(Route::Applet(h)) => h.id == 39 && h.argc == 2,
        _ => false,
    };
    let d = resolve(&[b"/bin/sh"]).map(|r| r.applet_id()) == Ok(Some(0));
    let e = resolve(&[b"/usr/bin/gerp"]).err() == Some(AD_EUNKNOWN);
    a && b && c && d && e
}

/// 多调形态面。
fn multi_ok() -> bool {
    let a = match resolve(&[N_BUSYBOX, N_CAT, b"-n"]) {
        Ok(Route::Applet(h)) => {
            h.id == 2 && h.origin == Origin::Multi && h.skip == 2 && h.argc == 1
        }
        _ => false,
    };
    let b = match resolve(&[N_BIN_BUSYBOX, b"ls", b"-l", b"/tmp", b"/var"]) {
        Ok(Route::Applet(h)) => h.skip == 2 && h.argc == 3 && h.id == 6,
        _ => false,
    };
    let c = match resolve(&[N_BUSYBOX, b"busybox"]) {
        Ok(_) => false,
        Err(c) => c == AD_EUNKNOWN,
    };
    let d = resolve(&[N_BUSYBOX]).err() == Some(AD_EEMPTY);
    let e = resolve(&[N_BUSYBOX, b""]).err() == Some(AD_EEMPTY);
    let f = resolve(&[N_BUSYBOX, &[b'g'; 33]]).err() == Some(AD_ELEN);
    a && b && c && d && e && f
}

/// 全局选项面。
fn global_ok() -> bool {
    let a = resolve(&[N_BUSYBOX, b"--list"]).ok().and_then(|r| r.global_name()) == Some("list");
    let b = resolve(&[N_BUSYBOX, b"--help"]).ok().and_then(|r| r.global_name()) == Some("help");
    let c = resolve(&[N_BUSYBOX, b"-h"]).ok().and_then(|r| r.global_name()) == Some("help");
    let d = resolve(&[N_BUSYBOX, b"-"]).err() == Some(AD_EBYTE);
    let e = resolve(&[N_BUSYBOX, b"-x"]).err() == Some(AD_EOPT);
    let f = resolve(&[N_BUSYBOX, b"--version"]).err() == Some(AD_EOPT);
    let g = resolve(&[N_BUSYBOX, b"--list"]).ok().and_then(|r| r.applet_id()).is_none();
    a && b && c && d && e && f && g
}

/// 未知与缺参面。
fn unknown_ok() -> bool {
    let a = resolve(&[]).err() == Some(AD_EEMPTY);
    let b = resolve(&[b"gerp"]).err() == Some(AD_EUNKNOWN);
    let c = resolve(&[b"ls", b"-x"]).map(|r| r.applet_id()) == Ok(Some(6));
    let d = resolve(&[&[0x00u8]]).err() == Some(AD_EBYTE);
    let e = resolve(&[b"busybox2"]).err() == Some(AD_EUNKNOWN);
    let f = resolve(&[b"sh/"]).err() == Some(AD_ETRAIL);
    let g = resolve(&[b".."]).err() == Some(AD_EDOT);
    a && b && c && d && e && f && g
}

/// 命中账面。
fn hits_ok() -> bool {
    let mut h = Hits::new();
    if h.top().is_some() || h.distinct() != 0 || h.total() != 0 {
        return false;
    }
    if h.note(9).is_err() {
        return false;
    }
    let _ = h.note(9);
    let _ = h.note(3);
    let _ = h.note(9);
    let _ = h.note(40).is_err();
    h.total() == 4
        && h.get(9) == 3
        && h.get(3) == 1
        && h.get(0) == 0
        && h.get(255) == 0
        && h.distinct() == 2
        && h.top() == Some(9)
        && h.text().as_str() == "#3=1 #9=3"
        && h.note_route(&Route::Global("list")).is_ok()
        && h.total() == 4
}

/// 子表面。
fn subtable_ok() -> bool {
    let t = match Table::from_names(&[N_LS, N_GREP, b"sh"]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if t.len() != 3 || t.is_empty() {
        return false;
    }
    if t.find(b"ls") != Some(6) || t.find(b"grep") != Some(17) || t.find(b"cat").is_some() {
        return false;
    }
    let r = match t.resolve(&[b"grep", b"x"]) {
        Ok(Route::Applet(h)) => h.id == 17 && h.skip == 1 && h.argc == 1,
        _ => false,
    };
    let m = match t.resolve(&[N_BUSYBOX, b"ls"]) {
        Ok(Route::Applet(h)) => h.id == 6 && h.origin == Origin::Multi,
        _ => false,
    };
    let dup = Table::from_names(&[N_LS, N_LS]).err() == Some(AD_EDUP);
    let unk = Table::from_names(&[b"nope"]).err() == Some(AD_EUNKNOWN);
    let cap = Table::from_names(&[N_LS; 41]).err() == Some(AD_ELEN);
    let at = t.at(2).map(|r| r.name) == Some("sh");
    let none = t.at(3).is_none();
    let text = t.names_text().as_str() == "ls grep sh";
    let cut = t.resolve(&[b"cat"]).err() == Some(AD_EUNKNOWN);
    r && m && dup && unk && cap && at && none && text && cut
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
    if l2.register(Sample::blank()) != Err(AD_ESLOT) {
        return false;
    }
    l2.rejects() == 1
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    if let Some(dst) = out.get_mut(0) {
        *dst = basename_ok() && byte_ok() && tail_ok();
    }
    if let Some(dst) = out.get_mut(1) {
        *dst = lookup_ok() && family_ok();
    }
    if let Some(dst) = out.get_mut(2) {
        *dst = direct_ok() && multi_ok() && global_ok() && unknown_ok();
    }
    if let Some(dst) = out.get_mut(3) {
        *dst = hits_ok() && subtable_ok() && ledger_ok();
    }
    out
}

/// 判据集合（13 项）。
pub fn run_unx11581_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11581.argv0");

    s.add("AD01-基名拆分", basename_ok(), "static");
    s.add("AD02-字节与长度闸", byte_ok(), "static");
    s.add("AD03-尾斜杠与点号", tail_ok(), "static");
    s.add("AD04-四十表查找", lookup_ok(), "static");
    s.add("AD05-族别计数", family_ok(), "static");
    s.add("AD06-直呼形态", direct_ok(), "static");
    s.add("AD07-多调形态", multi_ok(), "static");
    s.add("AD08-全局选项", global_ok(), "static");
    s.add("AD09-未知与缺参", unknown_ok(), "static");
    s.add("AD10-命中记账", hits_ok(), "static");
    s.add("AD11-子表过滤", subtable_ok(), "static");
    s.add("AD12-账本对账", build_ledger().audit() == Ok(()), "static");
    s.add(
        "AD13-冻结重放四注入",
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

    #[test]
    fn arg0_splits() {
        let a = Arg0::new(b"/bin/ls").expect("a");
        assert_eq!(a.dir_text().as_str(), "/bin");
        assert_eq!(a.base_text().as_str(), "ls");
        assert_eq!(a.dir_len(), 4);
        let b = Arg0::new(b"busybox").expect("b");
        assert_eq!(b.dir_len(), 0);
        assert!(b.is_busybox());
        let c = Arg0::new(b"/sbin/busybox").expect("c");
        assert!(c.is_busybox());
        assert_eq!(c.dir_text().as_str(), "/sbin");
    }

    #[test]
    fn arg0_gates() {
        assert_eq!(Arg0::new(b"").err(), Some(AD_EEMPTY));
        assert_eq!(Arg0::new(b"x/").err(), Some(AD_ETRAIL));
        assert_eq!(Arg0::new(b"/").err(), Some(AD_ETRAIL));
        assert_eq!(Arg0::new(b".").err(), Some(AD_EDOT));
        assert_eq!(Arg0::new(b"..").err(), Some(AD_EDOT));
        assert_eq!(Arg0::new(b"/bin/..").err(), Some(AD_EDOT));
        assert_eq!(Arg0::new(&[0x01]).err(), Some(AD_EBYTE));
        assert_eq!(Arg0::new(&[b'z'; 97]).err(), Some(AD_ELEN));
        assert_eq!(Arg0::new(&[b'z'; 33]).err(), Some(AD_ELEN));
        assert_eq!(Arg0::new(&[b'z'; 32]).map(|a| a.base_len()), Ok(32));
    }

    #[test]
    fn table_matches_f11204_order() {
        assert_eq!(APPLET_NAMES.len(), APPLETS);
        assert_eq!(APPLET_NAMES.first().copied(), Some("sh"));
        assert_eq!(APPLET_NAMES.last().copied(), Some("tar"));
        assert_eq!(lookup(b"ls").map(|r| r.id), Some(6));
        assert_eq!(lookup(b"grep").map(|r| r.id), Some(17));
        assert_eq!(lookup(b"dd").map(|r| r.id), Some(38));
        assert_eq!(lookup(b"tar").map(|r| r.id), Some(39));
        assert!(lookup(b"LS").is_none());
    }

    #[test]
    fn families_split() {
        assert_eq!(count_family(Family::Shell), 2);
        assert_eq!(count_family(Family::File), 16);
        assert_eq!(count_family(Family::Text), 14);
        assert_eq!(count_family(Family::Proc), 6);
        assert_eq!(count_family(Family::Misc), 2);
        let sum = count_family(Family::Shell)
            + count_family(Family::File)
            + count_family(Family::Text)
            + count_family(Family::Proc)
            + count_family(Family::Misc);
        assert_eq!(sum, APPLETS);
    }

    #[test]
    fn direct_route() {
        let r = resolve(&[b"/bin/ls", b"-l", b"/tmp"]).expect("r");
        assert_eq!(r.applet_id(), Some(6));
        match r {
            Route::Applet(h) => {
                assert_eq!(h.origin, Origin::Direct);
                assert_eq!(h.skip, 1);
                assert_eq!(h.argc, 2);
            }
            Route::Global(_) => panic!("not global"),
        }
        assert_eq!(resolve(&[b"ls"]).map(|r| r.applet_id()), Ok(Some(6)));
        assert_eq!(resolve(&[b"/usr/bin/gerp"]).err(), Some(AD_EUNKNOWN));
    }

    #[test]
    fn multi_route() {
        let r = resolve(&[b"/bin/busybox", b"cat", b"-n"]).expect("r");
        match r {
            Route::Applet(h) => {
                assert_eq!(h.id, 2);
                assert_eq!(h.origin, Origin::Multi);
                assert_eq!(h.skip, 2);
                assert_eq!(h.argc, 1);
            }
            Route::Global(_) => panic!("not global"),
        }
        assert_eq!(resolve(&[b"busybox"]).err(), Some(AD_EEMPTY));
        assert_eq!(resolve(&[b"busybox", b"-x"]).err(), Some(AD_EOPT));
    }

    #[test]
    fn global_routes() {
        assert_eq!(
            resolve(&[b"busybox", b"--list"]).ok().and_then(|r| r.global_name()),
            Some("list")
        );
        assert_eq!(
            resolve(&[b"busybox", b"--help"]).ok().and_then(|r| r.global_name()),
            Some("help")
        );
        assert_eq!(
            resolve(&[b"busybox", b"-h"]).ok().and_then(|r| r.global_name()),
            Some("help")
        );
        assert_eq!(resolve(&[b"busybox", b"-"]).err(), Some(AD_EBYTE));
    }

    #[test]
    fn unknown_diagnosed() {
        assert_eq!(resolve(&[]).err(), Some(AD_EEMPTY));
        assert_eq!(resolve(&[b"gerp"]).err(), Some(AD_EUNKNOWN));
        assert_eq!(resolve(&[b"busybox2"]).err(), Some(AD_EUNKNOWN));
        assert_eq!(resolve(&[b"sh/"]).err(), Some(AD_ETRAIL));
        assert_eq!(resolve(&[b".."]).err(), Some(AD_EDOT));
    }

    #[test]
    fn subtable_filters() {
        let t = Table::from_names(&[b"ls", b"grep", b"sh"]).expect("t");
        assert_eq!(t.len(), 3);
        assert_eq!(t.find(b"ls"), Some(6));
        assert_eq!(t.find(b"cat"), None);
        assert_eq!(t.names_text().as_str(), "ls grep sh");
        assert_eq!(t.resolve(&[b"cat"]).err(), Some(AD_EUNKNOWN));
        assert_eq!(Table::from_names(&[b"ls", b"ls"]).err(), Some(AD_EDUP));
        assert_eq!(Table::from_names(&[b"nope"]).err(), Some(AD_EUNKNOWN));
    }

    #[test]
    fn hits_count_and_top() {
        let mut h = Hits::new();
        assert!(h.top().is_none());
        let _ = h.note(6);
        let _ = h.note(6);
        let _ = h.note(17);
        assert_eq!(h.total(), 3);
        assert_eq!(h.distinct(), 2);
        assert_eq!(h.top(), Some(6));
        assert_eq!(h.text().as_str(), "#6=2 #17=1");
        assert_eq!(h.note(40).err(), Some(AD_EUNKNOWN));
        assert_eq!(h.get(40), 0);
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.sample(0).map(|s| s.value), Some(2));
        assert_eq!(l.sample(3).map(|s| s.value), Some(40));
        assert_eq!(l.sample(8).map(|s| s.code), Some(AD_ETRAIL));
        assert_eq!(l.sample(13).map(|s| s.code), Some(AD_EOPT));
        assert_eq!(l.sample(14), None);
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = l.freeze().expect("freeze");
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(AD_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(AD_ESLOT));
        assert_eq!(l.rejects(), 2);
        assert_eq!(l.root(), root);
        assert!(l.is_frozen());
    }

    #[test]
    fn ledger_rejects_bad_slot() {
        let mut l = Ledger::new();
        let s = Sample { slot: 3, op: OP_PARSE, ok: true, code: 0, value: 1 };
        assert_eq!(l.register(s), Err(AD_ESLOT));
        assert!(l.is_empty());
        assert_eq!(l.rejects(), 1);
        let ok = Sample { slot: 0, op: OP_PARSE, ok: true, code: 0, value: 1 };
        assert_eq!(l.register(ok), Ok(()));
        assert_eq!(l.len(), 1);
        assert_eq!(l.seal() == Ledger::new().seal(), false);
    }

    #[test]
    fn injections_all_true() {
        let four = inject_four();
        assert_eq!(four, [true, true, true, true]);
    }

    #[test]
    fn check_count_is_thirteen() {
        let s = run_unx11581_checks();
        assert_eq!(s.len(), 13);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11581_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}

