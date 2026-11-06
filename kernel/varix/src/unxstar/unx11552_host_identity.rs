//! UNX-F11552 hostid/logname/groups 身份杂件
//!
//! 三个「一行输出」的小工具共用一条身份链，所以放在同一张卡上：
//!
//! - **`hostid`**：把 32 位主机号打成 8 位小写十六进制。字节序是它唯一的坑——
//!   `0x0a0b0c0d` 打成 `0a0b0c0d`，从字节数组构造时按**大端**拼装，转换方向写反
//!   就会得到完全不同的数。解析接受可选 `0x` 前缀、大小写十六进制，但位数必须
//!   正好 8 位：`12`、`0x123` 都是长度错，`0xzzzzzzzz` 是字符错。
//! - **`logname`**：登录名解析链。先看 `LOGNAME` 环境变量，再看 utmp 里本终端的
//!   记录，最后退到 `USER`。三者都空就报不存在。环境变量与 utmp 同时存在但
//!   **不一致**时，是状态冲突而不是不存在——这条区别很重要：脚本里可以据此判断
//!   「环境被改写」而不是「没有登录」。
//! - **`groups`**：组表。输出的语义是「**主组在前，附加组按 gid 升序、去重**」，
//!   主组即使也出现在附加组里也只打一次。任何一个 gid 在组库里查不到都报不存在，
//!   不允许打印数字占位——数字对脚本没有意义，查不到就是配置坏了。
//!
//! 名字（用户名、组名）走同一套字节规则：非空、不超过 32 字节、只允许字母数字与
//! `_ - .`，且不许以 `-` 开头（否则会被当成选项）。终端名额外允许 `/`，但同样
//! 不许以 `/` 开头。
//!
//! 闸门次序：名字（`ID_EINVAL`/`ID_ELEN`/`ID_EBYTE`）→ 数值（`ID_ERANGE`）→
//! 组库（`ID_EDUP`/`ID_ENOENT`）→ 身份链（`ID_ESTATE`）。
//!
//! 账本语义：十四格 = 八格成功（两格主机号、两格组表、一格组计数、两格登录名、
//! 一格零主机号）+ 六格失败（长度、字符、未知组、重复组、无身份、身份冲突）。
//! `audit` 认「八值六码」，`seal` 做 FNV-1a 折叠，`replay` 重建比对，`freeze`
//! 后拒绝。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf390;

/// 参数不合法（空名、以 `-` 开头）。
pub const ID_EINVAL: u32 = SEG_BASE + 0x01;
/// 长度不对（名字超长、十六进制位数不对）。
pub const ID_ELEN: u32 = SEG_BASE + 0x02;
/// 字节不合法（名字字符、十六进制字符）。
pub const ID_EBYTE: u32 = SEG_BASE + 0x03;
/// 数值越界（容量）。
pub const ID_ERANGE: u32 = SEG_BASE + 0x04;
/// 组重复（gid 或组名）。
pub const ID_EDUP: u32 = SEG_BASE + 0x05;
/// 查不到（未知 gid、无登录名、找不到组）。
pub const ID_ENOENT: u32 = SEG_BASE + 0x06;
/// 环境变量不合法。
pub const ID_ENV: u32 = SEG_BASE + 0x07;
/// 状态冲突（环境与 utmp 不一致）。
pub const ID_ESTATE: u32 = SEG_BASE + 0x08;
/// 账本不平衡。
pub const ID_EBAL: u32 = SEG_BASE + 0x09;
/// 账本槽位不连续。
pub const ID_ESLOT: u32 = SEG_BASE + 0x0a;
/// 账本观测值不符。
pub const ID_EVALUE: u32 = SEG_BASE + 0x0b;
/// 重放指纹不符。
pub const ID_ESEAL: u32 = SEG_BASE + 0x0c;
/// 槽位没有观测。
pub const ID_ENOBS: u32 = SEG_BASE + 0x0d;

/// 名字容量。
pub const TAGCAP: usize = 32;
/// 组库容量。
pub const GCAP: usize = 24;
/// 附加组容量。
pub const SUPCAP: usize = 8;
/// 主机号十六进制位数。
pub const HEXDIGITS: usize = 8;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：主机号。
pub const OP_HOST: u8 = 1;
/// 操作码：组表。
pub const OP_GROUPS: u8 = 2;
/// 操作码：登录名。
pub const OP_LOGNAME: u8 = 3;
/// 操作码：账户。
pub const OP_ACCT: u8 = 4;
/// 操作码：解析。
pub const OP_PARSE: u8 = 5;
/// 操作码：计数。
pub const OP_COUNT: u8 = 6;

/// FNV-1a 偏移基。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 质数。
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 八字节折叠。
fn fold(mut h: u64, v: u64) -> u64 {
    let mut x = v;
    let mut i = 0usize;
    while i < 8 {
        h ^= x & 0xff;
        h = h.wrapping_mul(FNV_PRIME);
        x >>= 8;
        i += 1;
    }
    h
}

/// 十进制无符号。
fn push_u64(s: &mut String, mut v: u64) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 && n < 20 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut i = n;
    while i > 0 {
        i -= 1;
        if let Some(c) = buf.get(i) {
            s.push(*c as char);
        }
    }
}

/// 十六进制字符值。
fn hexval(b: u8) -> Option<u32> {
    if b.is_ascii_digit() {
        return Some((b - b'0') as u32);
    }
    if b >= b'a' && b <= b'f' {
        return Some((b - b'a') as u32 + 10);
    }
    if b >= b'A' && b <= b'F' {
        return Some((b - b'A') as u32 + 10);
    }
    None
}

/// 十六进制数字符。
fn hexchar(v: u32) -> char {
    let n = v & 0xf;
    if n < 10 {
        (b'0' + n as u8) as char
    } else {
        (b'a' + (n as u8 - 10)) as char
    }
}

/// 身份名字：非空、不超过 32 字节、只允许 `[A-Za-z0-9_.-]`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tag {
    buf: [u8; TAGCAP],
    len: u8,
}

impl Tag {
    /// 空名。
    pub const fn empty() -> Tag {
        Tag { buf: [0; TAGCAP], len: 0 }
    }

    /// 从字节解析。
    pub fn parse_bytes(raw: &[u8]) -> Result<Tag, u32> {
        if raw.is_empty() {
            return Err(ID_EINVAL);
        }
        if raw.len() > TAGCAP {
            return Err(ID_ELEN);
        }
        if let Some(first) = raw.first() {
            if *first == b'-' {
                return Err(ID_EINVAL);
            }
        }
        let mut i = 0usize;
        while i < raw.len() {
            let b = match raw.get(i) {
                Some(v) => *v,
                None => return Err(ID_EBYTE),
            };
            if !(b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.') {
                return Err(ID_EBYTE);
            }
            i += 1;
        }
        let mut buf = [0u8; TAGCAP];
        let mut k = 0usize;
        while k < raw.len() {
            if let (Some(dst), Some(src)) = (buf.get_mut(k), raw.get(k)) {
                *dst = *src;
            }
            k += 1;
        }
        Ok(Tag { buf, len: raw.len() as u8 })
    }

    /// 从字符串解析。
    pub fn parse(s: &str) -> Result<Tag, u32> {
        Tag::parse_bytes(s.as_bytes())
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 字节视图。
    pub fn as_bytes(&self) -> &[u8] {
        match self.buf.get(..self.len as usize) {
            Some(v) => v,
            None => &[],
        }
    }

    /// 与字节串比较。
    pub fn eq_bytes(&self, other: &[u8]) -> bool {
        self.as_bytes() == other
    }

    /// 与字符串比较。
    pub fn eq_str(&self, other: &str) -> bool {
        self.eq_bytes(other.as_bytes())
    }

    /// 追加。
    pub fn push_into(&self, s: &mut String) {
        let b = self.as_bytes();
        let mut i = 0usize;
        while i < b.len() {
            if let Some(c) = b.get(i) {
                s.push(*c as char);
            }
            i += 1;
        }
    }

    /// 转字符串。
    pub fn text(&self) -> String {
        let mut s = String::new();
        self.push_into(&mut s);
        s
    }
}

/// 终端名：非空、不超过 32 字节、允许 `/`，但不许以 `/` 或 `-` 开头。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tty {
    buf: [u8; TAGCAP],
    len: u8,
}

impl Tty {
    /// 空终端名。
    pub const fn empty() -> Tty {
        Tty { buf: [0; TAGCAP], len: 0 }
    }

    /// 从字节解析。
    pub fn parse_bytes(raw: &[u8]) -> Result<Tty, u32> {
        if raw.is_empty() {
            return Err(ID_EINVAL);
        }
        if raw.len() > TAGCAP {
            return Err(ID_ELEN);
        }
        // utmp 里存的是相对 `/dev/` 的名字，绝对路径说明记录已经被改写。
        if let Some(first) = raw.first() {
            if *first == b'/' || *first == b'-' {
                return Err(ID_EBYTE);
            }
        }
        let mut i = 0usize;
        while i < raw.len() {
            let b = match raw.get(i) {
                Some(v) => *v,
                None => return Err(ID_EBYTE),
            };
            if !(b.is_ascii_alphanumeric() || b == b'/' || b == b'_' || b == b'-' || b == b'.') {
                return Err(ID_EBYTE);
            }
            i += 1;
        }
        let mut buf = [0u8; TAGCAP];
        let mut k = 0usize;
        while k < raw.len() {
            if let (Some(dst), Some(src)) = (buf.get_mut(k), raw.get(k)) {
                *dst = *src;
            }
            k += 1;
        }
        Ok(Tty { buf, len: raw.len() as u8 })
    }

    /// 从字符串解析。
    pub fn parse(s: &str) -> Result<Tty, u32> {
        Tty::parse_bytes(s.as_bytes())
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 字节视图。
    pub fn as_bytes(&self) -> &[u8] {
        match self.buf.get(..self.len as usize) {
            Some(v) => v,
            None => &[],
        }
    }

    /// 与字节串比较。
    pub fn eq_bytes(&self, other: &[u8]) -> bool {
        self.as_bytes() == other
    }

    /// 追加。
    pub fn push_into(&self, s: &mut String) {
        let b = self.as_bytes();
        let mut i = 0usize;
        while i < b.len() {
            if let Some(c) = b.get(i) {
                s.push(*c as char);
            }
            i += 1;
        }
    }

    /// 转字符串。
    pub fn text(&self) -> String {
        let mut s = String::new();
        self.push_into(&mut s);
        s
    }
}

/// 32 位主机号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostId {
    value: u32,
}

impl HostId {
    /// 直接构造。
    pub const fn from_u32(v: u32) -> HostId {
        HostId { value: v }
    }

    /// 大端四字节构造。
    pub fn from_bytes(b: [u8; 4]) -> HostId {
        let v = (b[0] as u32) << 24 | (b[1] as u32) << 16 | (b[2] as u32) << 8 | b[3] as u32;
        HostId { value: v }
    }

    /// 数值。
    pub fn value(&self) -> u32 {
        self.value
    }

    /// 大端四字节。
    pub fn octets(&self) -> [u8; 4] {
        [
            (self.value >> 24) as u8,
            (self.value >> 16) as u8,
            (self.value >> 8) as u8,
            self.value as u8,
        ]
    }

    /// 是否为零。
    pub fn is_zero(&self) -> bool {
        self.value == 0
    }

    /// 是否是回环地址 127.0.0.1。
    pub fn is_loopback(&self) -> bool {
        self.value == 0x7f00_0001
    }

    /// 八位小写十六进制。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < HEXDIGITS {
            let shift = (HEXDIGITS - 1 - i) * 4;
            s.push(hexchar(self.value >> shift));
            i += 1;
        }
        s
    }

    /// 解析：可选 `0x`，正好八位十六进制。
    pub fn parse_hex(raw: &str) -> Result<HostId, u32> {
        let b = raw.as_bytes();
        if b.is_empty() {
            return Err(ID_EINVAL);
        }
        let body = if b.len() >= 2 && b.get(0) == Some(&b'0') {
            match b.get(1) {
                Some(x) if *x == b'x' || *x == b'X' => match b.get(2..) {
                    Some(v) => v,
                    None => return Err(ID_EINVAL),
                },
                _ => b,
            }
        } else {
            b
        };
        if body.is_empty() {
            return Err(ID_EINVAL);
        }
        if body.len() != HEXDIGITS {
            return Err(ID_ELEN);
        }
        let mut v = 0u32;
        let mut i = 0usize;
        while i < body.len() {
            let c = match body.get(i) {
                Some(x) => *x,
                None => return Err(ID_EBYTE),
            };
            let d = match hexval(c) {
                Some(x) => x,
                None => return Err(ID_EBYTE),
            };
            v = v << 4 | (d & 0xf);
            i += 1;
        }
        Ok(HostId { value: v })
    }
}

/// 环境变量快照（LOGNAME / USER）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Env {
    logname: Tag,
    user: Tag,
    has_logname: bool,
    has_user: bool,
}

impl Env {
    /// 空环境。
    pub const fn blank() -> Env {
        Env { logname: Tag::empty(), user: Tag::empty(), has_logname: false, has_user: false }
    }

    /// 设置 `LOGNAME`。
    pub fn with_logname(mut self, s: &str) -> Result<Env, u32> {
        let t = Tag::parse(s)?;
        self.logname = t;
        self.has_logname = true;
        Ok(self)
    }

    /// 设置 `USER`。
    pub fn with_user(mut self, s: &str) -> Result<Env, u32> {
        let t = Tag::parse(s)?;
        self.user = t;
        self.has_user = true;
        Ok(self)
    }

    /// 取 `LOGNAME`。
    pub fn logname(&self) -> Option<Tag> {
        if self.has_logname {
            Some(self.logname)
        } else {
            None
        }
    }

    /// 取 `USER`。
    pub fn user(&self) -> Option<Tag> {
        if self.has_user {
            Some(self.user)
        } else {
            None
        }
    }
}

/// utmp 里属于本终端的一条记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Session {
    user: Tag,
    tty: Tty,
}

impl Session {
    /// 构造。
    pub fn new(user: &str, tty: &str) -> Result<Session, u32> {
        let u = Tag::parse(user)?;
        let t = Tty::parse(tty)?;
        Ok(Session { user: u, tty: t })
    }

    /// 用户名。
    pub fn user(&self) -> &Tag {
        &self.user
    }

    /// 终端。
    pub fn tty(&self) -> &Tty {
        &self.tty
    }
}

/// 登录名解析链：`LOGNAME` → utmp 本终端 → `USER`。
pub fn logname(env: &Env, own: Option<&Session>) -> Result<Tag, u32> {
    if let Some(l) = env.logname() {
        if !l.is_empty() {
            return Ok(l);
        }
    }
    if let Some(s) = own {
        if !s.user.is_empty() {
            return Ok(s.user);
        }
    }
    if let Some(u) = env.user() {
        if !u.is_empty() {
            return Ok(u);
        }
    }
    Err(ID_ENOENT)
}

/// 身份一致性：环境与 utmp 同时存在时必须同名。
pub fn consistency(env: &Env, own: Option<&Session>) -> Result<(), u32> {
    let l = env.logname();
    let s = match own {
        Some(v) => v,
        None => return Ok(()),
    };
    match l {
        Some(t) => {
            if t.eq_bytes(s.user.as_bytes()) {
                Ok(())
            } else {
                Err(ID_ESTATE)
            }
        }
        None => Ok(()),
    }
}

/// 组定义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Group {
    name: Tag,
    gid: u32,
}

impl Group {
    /// 空白组。
    pub const fn blank() -> Group {
        Group { name: Tag::empty(), gid: 0 }
    }

    /// 构造。
    pub fn new(name: &str, gid: u32) -> Result<Group, u32> {
        let n = Tag::parse(name)?;
        Ok(Group { name: n, gid })
    }

    /// 组名。
    pub fn name(&self) -> &Tag {
        &self.name
    }

    /// gid。
    pub fn gid(&self) -> u32 {
        self.gid
    }
}

/// 组库。
#[derive(Clone, Copy, Debug)]
pub struct GroupDb {
    rows: [Group; GCAP],
    len: usize,
}

impl GroupDb {
    /// 空库。
    pub const fn new() -> GroupDb {
        GroupDb { rows: [Group::blank(); GCAP], len: 0 }
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 条。
    pub fn at(&self, i: usize) -> Option<Group> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 登记（gid 与组名都不许重复）。
    pub fn add(&mut self, g: Group) -> Result<(), u32> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(x) = self.rows.get(i) {
                if x.gid == g.gid || x.name.eq_bytes(g.name.as_bytes()) {
                    return Err(ID_EDUP);
                }
            }
            i += 1;
        }
        if self.len >= GCAP {
            return Err(ID_ERANGE);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = g;
        }
        self.len += 1;
        Ok(())
    }

    /// 按 gid 查。
    pub fn find_gid(&self, gid: u32) -> Option<Group> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(x) = self.rows.get(i) {
                if x.gid == gid {
                    return Some(*x);
                }
            }
            i += 1;
        }
        None
    }

    /// 按组名查。
    pub fn find_name(&self, name: &str) -> Option<Group> {
        let want = Tag::parse(name).ok()?;
        let mut i = 0usize;
        while i < self.len {
            if let Some(x) = self.rows.get(i) {
                if x.name.eq_bytes(want.as_bytes()) {
                    return Some(*x);
                }
            }
            i += 1;
        }
        None
    }

    /// 组名 → gid。
    pub fn gid_of(&self, name: &str) -> Option<u32> {
        self.find_name(name).map(|g| g.gid)
    }

    /// gid → 组名。
    pub fn name_of(&self, gid: u32) -> Option<Tag> {
        self.find_gid(gid).map(|g| *g.name())
    }
}

/// 账户：主组 + 附加组。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Account {
    user: Tag,
    primary: u32,
    supp: [u32; SUPCAP],
    n: usize,
}

impl Account {
    /// 构造。
    pub fn new(user: &str, primary: u32) -> Result<Account, u32> {
        let u = Tag::parse(user)?;
        Ok(Account { user: u, primary, supp: [0; SUPCAP], n: 0 })
    }

    /// 用户名。
    pub fn user(&self) -> &Tag {
        &self.user
    }

    /// 主组。
    pub fn primary(&self) -> u32 {
        self.primary
    }

    /// 附加组切片。
    pub fn supp(&self) -> &[u32] {
        match self.supp.get(..self.n) {
            Some(v) => v,
            None => &[],
        }
    }

    /// 追加附加组（重复报重、满报范围错）。
    pub fn add_supp(&mut self, gid: u32) -> Result<(), u32> {
        if gid == self.primary {
            return Err(ID_EDUP);
        }
        let mut i = 0usize;
        while i < self.n {
            if self.supp.get(i) == Some(&gid) {
                return Err(ID_EDUP);
            }
            i += 1;
        }
        if self.n >= SUPCAP {
            return Err(ID_ERANGE);
        }
        if let Some(dst) = self.supp.get_mut(self.n) {
            *dst = gid;
        }
        self.n += 1;
        Ok(())
    }

    /// 是否属于某组。
    pub fn is_member(&self, gid: u32) -> bool {
        if gid == self.primary {
            return true;
        }
        let mut i = 0usize;
        while i < self.n {
            if self.supp.get(i) == Some(&gid) {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 附加组条数。
    pub fn supp_len(&self) -> usize {
        self.n
    }
}

/// 组表渲染：主组在前，附加组升序去重。
pub fn groups_render(db: &GroupDb, acct: &Account) -> Result<String, u32> {
    let primary = match db.find_gid(acct.primary) {
        Some(g) => g,
        None => return Err(ID_ENOENT),
    };
    let mut s = String::new();
    primary.name().push_into(&mut s);
    let mut ids = [0u32; SUPCAP];
    let mut n = 0usize;
    let supp = acct.supp();
    let mut i = 0usize;
    while i < supp.len() {
        if let Some(g) = supp.get(i) {
            if *g != acct.primary {
                if let Some(dst) = ids.get_mut(n) {
                    *dst = *g;
                }
                n += 1;
            }
        }
        i += 1;
    }
    let mut a = 1usize;
    while a < n {
        let mut j = a;
        while j > 0 {
            let x = match ids.get(j - 1) {
                Some(v) => *v,
                None => break,
            };
            let y = match ids.get(j) {
                Some(v) => *v,
                None => break,
            };
            if x <= y {
                break;
            }
            ids[j - 1] = y;
            ids[j] = x;
            j -= 1;
        }
        a += 1;
    }
    let mut last: Option<u32> = None;
    let mut k = 0usize;
    while k < n {
        let gid = match ids.get(k) {
            Some(v) => *v,
            None => break,
        };
        if last == Some(gid) {
            k += 1;
            continue;
        }
        let g = match db.find_gid(gid) {
            Some(v) => v,
            None => return Err(ID_ENOENT),
        };
        s.push(' ');
        g.name().push_into(&mut s);
        last = Some(gid);
        k += 1;
    }
    Ok(s)
}

/// 组表名单个数（与渲染一致）。
pub fn groups_count(db: &GroupDb, acct: &Account) -> Result<usize, u32> {
    let line = groups_render(db, acct)?;
    let mut n = 0usize;
    let mut in_word = false;
    let b = line.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        match b.get(i) {
            Some(c) => {
                if *c == b' ' {
                    in_word = false;
                } else if !in_word {
                    in_word = true;
                    n += 1;
                }
            }
            None => break,
        }
        i += 1;
    }
    Ok(n)
}

/// 核对一行组表。
pub fn groups_match(line: &str, db: &GroupDb, acct: &Account) -> Result<(), u32> {
    let want = groups_render(db, acct)?;
    if want == line {
        Ok(())
    } else {
        Err(ID_EVALUE)
    }
}

/// 标准组库。
fn build_db() -> GroupDb {
    let mut db = GroupDb::new();
    let rows: [(&str, u32); 6] =
        [("root", 0), ("daemon", 1), ("sys", 4), ("sudo", 27), ("staff", 50), ("users", 100)];
    let mut i = 0usize;
    while i < rows.len() {
        if let Some((name, gid)) = rows.get(i) {
            if let Ok(g) = Group::new(name, *gid) {
                let _ = db.add(g);
            }
        }
        i += 1;
    }
    db
}

/// 标准账户 alice：主组 staff(50)，附加 sudo(27)、sys(4)。
fn build_alice() -> Account {
    let mut a = match Account::new("alice", 50) {
        Ok(v) => v,
        Err(_) => Account { user: Tag::empty(), primary: 0, supp: [0; SUPCAP], n: 0 },
    };
    let _ = a.add_supp(27);
    let _ = a.add_supp(4);
    a
}

/// 标准账户 bob：主组 users(100)，附加 staff(50)、sys(4)。
fn build_bob() -> Account {
    let mut a = match Account::new("bob", 100) {
        Ok(v) => v,
        Err(_) => Account { user: Tag::empty(), primary: 0, supp: [0; SUPCAP], n: 0 },
    };
    let _ = a.add_supp(50);
    let _ = a.add_supp(4);
    a
}

/// 账本样本。
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

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<usize, u32>` → 样本。
fn count_cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 样本账本。
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

    /// 登记一格。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ID_ESLOT);
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

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 格。
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

    /// 对账：八成功值 + 六失败码。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(ID_EBAL);
        }
        const VALS: [u32; 8] = [8, 2_130_706_433, 14, 3, 5, 3, 8, 15];
        const CODES: [u32; 6] = [ID_ELEN, ID_EBYTE, ID_ENOENT, ID_EDUP, ID_ENOENT, ID_ESTATE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(ID_EBAL),
            };
            if s.slot as usize != i {
                return Err(ID_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(ID_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(ID_EVALUE);
                        }
                    }
                    None => return Err(ID_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(ID_EVALUE);
                        }
                    }
                    None => return Err(ID_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(ID_EBAL);
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
            return Err(ID_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
        } else {
            Err(ID_ESEAL)
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

/// 十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let db = build_db();
    let alice = build_alice();
    let bob = build_bob();

    let _ = l.register(word_cell(0, OP_HOST, Ok(HostId::from_u32(0x0a0b_0c0d).render().len() as u32)));
    let _ = l.register(word_cell(1, OP_HOST, HostId::parse_hex("0x7f000001").map(|h| h.value())));
    let _ = l.register(word_cell(2, OP_GROUPS, groups_render(&db, &alice).map(|s| s.len() as u32)));
    let _ = l.register(count_cell(3, OP_COUNT, groups_count(&db, &alice)));

    let env_alice = match Env::blank().with_logname("alice") {
        Ok(v) => v,
        Err(_) => Env::blank(),
    };
    let _ = l.register(word_cell(4, OP_LOGNAME, logname(&env_alice, None).map(|t| t.len() as u32)));

    let sess_bob = Session::new("bob", "pts/3");
    let _ = l.register(word_cell(5, OP_LOGNAME, match sess_bob {
        Ok(s) => logname(&Env::blank(), Some(&s)).map(|t| t.len() as u32),
        Err(c) => Err(c),
    }));

    let _ = l.register(word_cell(6, OP_HOST, Ok(HostId::from_u32(0).render().len() as u32)));
    let _ = l.register(word_cell(7, OP_GROUPS, groups_render(&db, &bob).map(|s| s.len() as u32)));

    let _ = l.register(word_cell(8, OP_HOST, HostId::parse_hex("12").map(|h| h.value())));
    let _ = l.register(word_cell(9, OP_HOST, HostId::parse_hex("0xzzzzzzzz").map(|h| h.value())));

    let mut bad = build_alice();
    let _ = bad.add_supp(999);
    let _ = l.register(word_cell(10, OP_GROUPS, groups_render(&db, &bad).map(|s| s.len() as u32)));

    let mut db2 = build_db();
    let _ = l.register(word_cell(11, OP_GROUPS, Group::new("staff2", 50).map(|g| match db2.add(g) {
        Ok(()) => 0u32,
        Err(c) => c,
    }).and_then(|v| if v == 0 { Ok(0) } else { Err(v) })));

    let _ = l.register(word_cell(12, OP_LOGNAME, logname(&Env::blank(), None).map(|t| t.len() as u32)));

    let _ = l.register(word_cell(13, OP_LOGNAME, match sess_bob {
        Ok(s) => consistency(&env_alice, Some(&s)).map(|_| 0),
        Err(c) => Err(c),
    }));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：主机号的三个方向。
    let a = HostId::from_u32(0x0a0b_0c0d).render() == String::from("0a0b0c0d");
    let b = HostId::from_bytes([0x7f, 0x00, 0x00, 0x01]).value() == 0x7f00_0001;
    let c = HostId::from_u32(0x0102_0304).octets() == [1, 2, 3, 4];
    let d = HostId::parse_hex("0x7F000001").map(|h| h.is_loopback()) == Ok(true);
    let e = HostId::from_u32(0).is_zero() && HostId::from_u32(0).render() == String::from("00000000");
    out[1] = a && b && c && d && e;

    // 二路：组表的顺序、去重与拒绝。
    let db = build_db();
    let alice = build_alice();
    let bob = build_bob();
    let a2 = groups_render(&db, &alice) == Ok(String::from("staff sys sudo"));
    let b2 = groups_render(&db, &bob) == Ok(String::from("users sys staff"));
    let c2 = groups_count(&db, &alice) == Ok(3) && groups_count(&db, &bob) == Ok(3);
    let d2 = groups_match("staff sys sudo", &db, &alice).is_ok()
        && groups_match("staff  sys sudo", &db, &alice) == Err(ID_EVALUE);
    let mut unknown = build_alice();
    let _ = unknown.add_supp(999);
    let e2 = groups_render(&db, &unknown) == Err(ID_ENOENT);
    let dup = Group::new("staff2", 50).map(|g| {
        let mut d = build_db();
        d.add(g) == Err(ID_EDUP)
    });
    out[2] = a2 && b2 && c2 && d2 && e2 && dup == Ok(true);

    // 三路：登录名链与身份一致性。
    let env_a = match Env::blank().with_logname("alice") {
        Ok(v) => v,
        Err(_) => Env::blank(),
    };
    let sess_b = Session::new("bob", "pts/3").ok();
    let sess_a = Session::new("alice", "pts/2").ok();
    let f1 = logname(&env_a, sess_a.as_ref()) == Ok(match Tag::parse("alice") {
        Ok(t) => t,
        Err(_) => Tag::empty(),
    });
    let f2 = match sess_b.as_ref() {
        Some(s) => logname(&Env::blank(), Some(s)) == Ok(match Tag::parse("bob") {
            Ok(t) => t,
            Err(_) => Tag::empty(),
        }),
        None => false,
    };
    let f3 = logname(&Env::blank(), None) == Err(ID_ENOENT);
    let f4 = match env_a.with_user("carol") {
        Ok(e) => logname(&e, None) == Ok(match Tag::parse("alice") {
            Ok(t) => t,
            Err(_) => Tag::empty(),
        }),
        Err(_) => false,
    };
    let f5 = match sess_b.as_ref() {
        Some(s) => consistency(&env_a, Some(s)) == Err(ID_ESTATE)
            && consistency(&env_a, sess_a.as_ref()) == Ok(()),
        None => false,
    };

    let mut l2 = build_ledger();
    let one = l2.freeze();
    let rep2 = l2.replay();
    let two = l2.freeze();
    let reg = l2.register(Sample::blank());
    out[3] = f1
        && f2
        && f3
        && f4
        && f5
        && one.is_ok()
        && rep2 == one
        && two.is_err()
        && reg == Err(ID_ESLOT)
        && l2.is_frozen()
        && l2.rejects() == 2;

    out
}

/// 十三项判据。
pub fn run_unx11552_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11552.hostid");

    s.add(
        "ID01-主机号十六进制渲染",
        {
            let a = HostId::from_u32(0x0a0b_0c0d).render() == String::from("0a0b0c0d");
            let b = HostId::from_u32(0).render() == String::from("00000000");
            let c = HostId::from_u32(0xffff_ffff).render() == String::from("ffffffff");
            let d = HostId::from_bytes([1, 2, 3, 4]).value() == 0x0102_0304;
            let e = HostId::from_u32(0x1122_3344).octets() == [0x11, 0x22, 0x33, 0x44];
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "ID02-主机号解析闸门",
        {
            HostId::parse_hex("0x7f000001").map(|h| h.value()) == Ok(0x7f00_0001)
                && HostId::parse_hex("7F000001").map(|h| h.value()) == Ok(0x7f00_0001)
                && HostId::parse_hex("0xdeadbeef").map(|h| h.value()) == Ok(0xdead_beef)
                && HostId::parse_hex("") == Err(ID_EINVAL)
                && HostId::parse_hex("0x") == Err(ID_EINVAL)
                && HostId::parse_hex("12") == Err(ID_ELEN)
                && HostId::parse_hex("0x123") == Err(ID_ELEN)
                && HostId::parse_hex("0xzzzzzzzz") == Err(ID_EBYTE)
                && HostId::parse_hex("0xg0000000") == Err(ID_EBYTE)
        },
        "static",
    );

    s.add(
        "ID03-组库登记去重",
        {
            let mut db = GroupDb::new();
            let a = db.is_empty() && db.len() == 0;
            let g1 = Group::new("staff", 50);
            let b = match g1 {
                Ok(g) => db.add(g) == Ok(()),
                Err(_) => false,
            };
            let g2 = Group::new("users", 100);
            let c = match g2 {
                Ok(g) => db.add(g) == Ok(()) && db.len() == 2,
                Err(_) => false,
            };
            let g3 = Group::new("staff2", 50);
            let d = match g3 {
                Ok(g) => db.add(g) == Err(ID_EDUP),
                Err(_) => false,
            };
            let g4 = Group::new("staff", 51);
            let e = match g4 {
                Ok(g) => db.add(g) == Err(ID_EDUP),
                Err(_) => false,
            };
            let f = Group::new("", 60) == Err(ID_EINVAL)
                && Group::new("-x", 61) == Err(ID_EINVAL)
                && Group::new("a b", 62) == Err(ID_EBYTE);
            a && b && c && d && e && f
        },
        "static",
    );

    s.add(
        "ID04-组查找",
        {
            let db = build_db();
            let a = db.find_gid(50).map(|g| g.name().eq_bytes(b"staff")) == Some(true);
            let b = db.find_name("users").map(|g| g.gid()) == Some(100);
            let c = db.gid_of("sudo") == Some(27);
            let d = db.name_of(4).map(|t| t.eq_bytes(b"sys")) == Some(true);
            let e = db.find_gid(999).is_none() && db.find_name("nope").is_none();
            let f = db.len() == 6 && db.at(0).map(|g| g.gid()) == Some(0) && db.at(6).is_none();
            a && b && c && d && e && f
        },
        "static",
    );

    s.add(
        "ID05-组表主组先行与升序去重",
        {
            let db = build_db();
            let alice = build_alice();
            let bob = build_bob();
            let a = groups_render(&db, &alice) == Ok(String::from("staff sys sudo"));
            let b = groups_render(&db, &bob) == Ok(String::from("users sys staff"));
            let c = groups_count(&db, &alice) == Ok(3) && groups_count(&db, &bob) == Ok(3);
            let mut one = match Account::new("carol", 100) {
                Ok(v) => v,
                Err(_) => build_alice(),
            };
            let _ = one.add_supp(50);
            let _ = one.add_supp(50);
            let d = one.add_supp(50) == Err(ID_EDUP);
            let e = groups_render(&db, &one) == Ok(String::from("users staff"));
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "ID06-组表拒绝未知组",
        {
            let db = build_db();
            let mut a = build_alice();
            let _ = a.add_supp(999);
            let mut b = match Account::new("dave", 777) {
                Ok(v) => v,
                Err(_) => build_alice(),
            };
            let _ = b.add_supp(4);
            groups_render(&db, &a) == Err(ID_ENOENT)
                && groups_count(&db, &a) == Err(ID_ENOENT)
                && groups_match("x", &db, &a) == Err(ID_ENOENT)
                && groups_render(&db, &b) == Err(ID_ENOENT)
        },
        "static",
    );

    s.add(
        "ID07-登录名优先链",
        {
            let env_a = Env::blank().with_logname("alice");
            let env_u = Env::blank().with_user("dave");
            let sess_b = Session::new("bob", "pts/3").ok();
            let a = match env_a {
                Ok(e) => logname(&e, sess_b.as_ref()).map(|t| t.text()) == Ok(String::from("alice")),
                Err(_) => false,
            };
            let b = match sess_b.as_ref() {
                Some(s) => logname(&Env::blank(), Some(s)).map(|t| t.text()) == Ok(String::from("bob")),
                None => false,
            };
            let c = match env_u {
                Ok(e) => {
                    logname(&e, None).map(|t| t.text()) == Ok(String::from("dave"))
                        && logname(&e, sess_b.as_ref()).map(|t| t.text()) == Ok(String::from("bob"))
                }
                Err(_) => false,
            };
            let d = logname(&Env::blank(), None) == Err(ID_ENOENT);
            let e = Env::blank().logname().is_none()
                && Env::blank().with_logname("").is_err()
                && Env::blank().with_user("-bad").is_err();
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "ID08-登录名一致性",
        {
            let env_a = Env::blank().with_logname("alice");
            let env_b = Env::blank().with_logname("bob");
            let sess_a = Session::new("alice", "pts/2").ok();
            let sess_b = Session::new("bob", "pts/3").ok();
            let a = match env_a {
                Ok(e) => consistency(&e, sess_a.as_ref()) == Ok(()),
                Err(_) => false,
            };
            let b = match (env_a, sess_b.as_ref()) {
                (Ok(e), Some(s)) => consistency(&e, Some(s)) == Err(ID_ESTATE),
                _ => false,
            };
            let c = match env_b {
                Ok(e) => consistency(&e, None) == Ok(()),
                Err(_) => false,
            };
            let d = consistency(&Env::blank(), sess_b.as_ref()) == Ok(());
            a && b && c && d
        },
        "static",
    );

    s.add(
        "ID09-账户成员判定",
        {
            let a = build_alice();
            let b = build_bob();
            a.is_member(50)
                && a.is_member(4)
                && a.is_member(27)
                && !a.is_member(100)
                && b.is_member(100)
                && b.is_member(50)
                && !b.is_member(27)
                && a.primary() == 50
                && a.supp_len() == 2
                && b.user().eq_bytes(b"bob")
        },
        "static",
    );

    s.add(
        "ID10-会话名解析",
        {
            let a = Session::new("alice", "pts/2");
            let b = Session::new("alice", "/dev/pts/2");
            let c = Session::new("alice", "");
            let d = Session::new("alice", "pts 2");
            let e = Session::new("alice", "abcdefghijklmnopqrstuvwxyz0123456789");
            let f = Session::new("", "pts/2");
            a.map(|s| s.user().eq_bytes(b"alice") && s.tty().eq_bytes(b"pts/2")) == Ok(true)
                && b == Err(ID_EBYTE)
                && c == Err(ID_EINVAL)
                && d == Err(ID_EBYTE)
                && e == Err(ID_ELEN)
                && f == Err(ID_EINVAL)
        },
        "static",
    );

    s.add(
        "ID11-三件单行输出",
        {
            let host = HostId::from_u32(0x0a0b_0c0d).render();
            let db = build_db();
            let alice = build_alice();
            let groups = groups_render(&db, &alice);
            let lg = logname(&Env::blank().with_user("alice").unwrap_or(Env::blank()), None);
            let a = host.len() == 8 && !host.contains('\n');
            let b = match groups {
                Ok(ref g) => g == "staff sys sudo" && !g.contains('\n') && g.len() <= 64,
                Err(_) => false,
            };
            let c = match lg {
                Ok(t) => t.eq_bytes(b"alice") && t.text().len() == 5,
                Err(_) => false,
            };
            let d = groups_match("staff sys sudo", &db, &alice).is_ok()
                && groups_match("staff sys  sudo", &db, &alice).is_err();
            a && b && c && d
        },
        "static",
    );

    s.add(
        "ID12-账本审计",
        {
            let l = build_ledger();
            l.len() == SAMPLES
                && l.ok_count() == 8
                && l.err_count() == 6
                && l.audit().is_ok()
                && l.rejects() == 0
                && l.sample(0).map(|x| x.value) == Some(8)
                && l.sample(1).map(|x| x.value) == Some(0x7f00_0001)
                && l.sample(2).map(|x| x.value) == Some(14)
                && l.sample(13).map(|x| x.code) == Some(ID_ESTATE)
                && l.sample(14).is_none()
        },
        "static",
    );

    s.add(
        "ID13-冻结重放四注入",
        inject_four() == [true; 4] && build_ledger().audit().is_ok(),
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostid_vectors() {
        assert_eq!(HostId::from_u32(0x0a0b_0c0d).render(), String::from("0a0b0c0d"));
        assert_eq!(HostId::from_u32(0).render(), String::from("00000000"));
        assert_eq!(HostId::from_u32(0xdead_beef).render(), String::from("deadbeef"));
        assert_eq!(HostId::from_bytes([0x7f, 0, 0, 1]).value(), 0x7f00_0001);
        assert!(HostId::from_bytes([0x7f, 0, 0, 1]).is_loopback());
        assert_eq!(HostId::from_u32(0x0102_0304).octets(), [1, 2, 3, 4]);
        assert!(HostId::from_u32(0).is_zero());
    }

    #[test]
    fn hostid_parse_gates() {
        assert_eq!(HostId::parse_hex("0x7f000001").map(|h| h.value()), Ok(0x7f00_0001));
        assert_eq!(HostId::parse_hex("7F000001").map(|h| h.value()), Ok(0x7f00_0001));
        assert_eq!(HostId::parse_hex("0XDEADBEEF").map(|h| h.value()), Ok(0xdead_beef));
        assert_eq!(HostId::parse_hex(""), Err(ID_EINVAL));
        assert_eq!(HostId::parse_hex("0x"), Err(ID_EINVAL));
        assert_eq!(HostId::parse_hex("12"), Err(ID_ELEN));
        assert_eq!(HostId::parse_hex("0x123456789"), Err(ID_ELEN));
        assert_eq!(HostId::parse_hex("0xzzzzzzzz"), Err(ID_EBYTE));
    }

    #[test]
    fn tag_gates() {
        assert_eq!(Tag::parse("alice").map(|t| t.len()), Ok(5));
        assert_eq!(Tag::parse("_a.b-c").map(|t| t.len()), Ok(6));
        assert_eq!(Tag::parse(""), Err(ID_EINVAL));
        assert_eq!(Tag::parse("-alice"), Err(ID_EINVAL));
        assert_eq!(Tag::parse("a b"), Err(ID_EBYTE));
        assert_eq!(Tag::parse("a:b"), Err(ID_EBYTE));
        assert_eq!(Tag::parse("abcdefghijklmnopqrstuvwxyz0123456789"), Err(ID_ELEN));
        assert!(Tag::parse("alice").map(|t| t.eq_str("alice")) == Ok(true));
    }

    #[test]
    fn group_db_ops() {
        let mut db = GroupDb::new();
        assert!(db.is_empty());
        let a = Group::new("staff", 50);
        assert!(a.is_ok());
        if let Ok(g) = a {
            assert_eq!(db.add(g), Ok(()));
            assert_eq!(db.add(g), Err(ID_EDUP));
        }
        let b = Group::new("users", 100);
        if let Ok(g) = b {
            assert_eq!(db.add(g), Ok(()));
        }
        assert_eq!(db.len(), 2);
        assert_eq!(db.gid_of("users"), Some(100));
        assert_eq!(db.name_of(50).map(|t| t.text()), Some(String::from("staff")));
        assert!(db.find_gid(7).is_none());
    }

    #[test]
    fn account_and_groups() {
        let db = build_db();
        let alice = build_alice();
        assert_eq!(groups_render(&db, &alice), Ok(String::from("staff sys sudo")));
        assert_eq!(groups_count(&db, &alice), Ok(3));
        assert!(alice.is_member(4) && alice.is_member(27) && alice.is_member(50));
        assert!(!alice.is_member(100));
        let bob = build_bob();
        assert_eq!(groups_render(&db, &bob), Ok(String::from("users sys staff")));
        let mut bad = build_alice();
        assert_eq!(bad.add_supp(999), Ok(()));
        assert_eq!(groups_render(&db, &bad), Err(ID_ENOENT));
    }

    #[test]
    fn logname_chain() {
        let env = Env::blank().with_logname("alice");
        assert!(env.is_ok());
        if let Ok(e) = env {
            assert_eq!(logname(&e, None).map(|t| t.text()), Ok(String::from("alice")));
        }
        let sess = Session::new("bob", "pts/3");
        assert!(sess.is_ok());
        if let Ok(s) = sess {
            assert_eq!(logname(&Env::blank(), Some(&s)).map(|t| t.text()), Ok(String::from("bob")));
            let e2 = Env::blank().with_logname("alice");
            if let Ok(e) = e2 {
                assert_eq!(consistency(&e, Some(&s)), Err(ID_ESTATE));
            }
        }
        assert_eq!(logname(&Env::blank(), None), Err(ID_ENOENT));
        assert_eq!(consistency(&Env::blank(), None), Ok(()));
    }

    #[test]
    fn session_gates() {
        assert!(Session::new("alice", "pts/2").is_ok());
        assert_eq!(Session::new("alice", "/dev/pts/2"), Err(ID_EBYTE));
        assert_eq!(Session::new("alice", ""), Err(ID_EINVAL));
        assert_eq!(Session::new("alice", "pts 2"), Err(ID_EBYTE));
        assert_eq!(Session::new("alice", "abcdefghijklmnopqrstuvwxyz0123456789"), Err(ID_ELEN));
        assert_eq!(Session::new("", "pts/2"), Err(ID_EINVAL));
        assert_eq!(Session::new("a:b", "pts/2"), Err(ID_EBYTE));
    }

    #[test]
    fn ledger_flow() {
        let mut l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert!(l.audit().is_ok());
        let root = l.seal();
        assert_ne!(root, 0);
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Ok(root));
        assert_eq!(l.freeze(), Err(ID_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(ID_ESLOT));
        assert_eq!(l.rejects(), 2);
        assert_eq!(l.root(), root);
        assert!(l.is_frozen());
    }
}
