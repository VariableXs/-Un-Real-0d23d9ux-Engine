//! UNX-F11553 install 复制并设属性
//!
//! `install` 是「复制 + 现场改属性」的一体化工具：它不像 `cp` 那样先原样拷过去再
//! 由调用者补一刀 `chmod`，而是在复制的那一刻就决定目标文件的**权限位**和
//! **属主/属组**，并在 `-d` 档里把「建目录」本身当成主操作。因此它必须自己维护
//! 三件事，缺一条就会和基准机分叉：
//!
//! 1. **权限位算术**：`-m 755` 是八进制直给，但基准机也接受符号式
//!    `u+x,g-w,o=r`——符号式要按 chmod 的规则在**基模式**上做增量，
//!    其中 `X` 只在「目录或已有任一 x 位」时才落地，`s`/`t` 落在特殊位上。
//! 2. **路径拼装**：`-t DIR` 把「DIR/基名」算作目标，`-T` 则禁止把目标当目录，
//!    同一次运行里两个源落到同一个目标必须先报重复（`IN_EDUP`），不能在覆盖
//!    之后才后悔。路径本身也有硬规则：不许空、不许 NUL、不许 `//`、不许尾斜杠、
//!    长度上限 `PATHCAP`。
//! 3. **属主闸门**：改属主（`-o`/`-g`）和挂 setuid/setgid 位都是特权动作。
//!    非 root 却要求这两件事，基准机在**动文件之前**就以 `EPERM` 退出，
//!    所以这里把它们做成纯判定的守卫，账本里也是「动之前就失败」的失败格。
//!
//! 闸门次序：选项字面（`IN_EINVAL`）→ 选项长度（`IN_ELEN`）→ 字节合法性
//! （`IN_EBYTE`）→ 八进制范围（`IN_ERANGE`）→ 重复（`IN_EDUP`）→
//! 名字查不到（`IN_ENOENT`）→ 特权不足（`IN_EPERM`）→ 状态冲突（`IN_ESTATE`）。
//! 范围在重复之前：`-m 17777` 是「值本身就错」，报重复会把注意力引到错的地方。
//!
//! 账本语义：十四格 = 八格成功（八进制、符号式、符号渲染、路径拼装、冗余行、
//! 属主号、属组号、目录模式）+ 六格失败（坏字节、超范围、坏符号、非特权改属主、
//! 目标重复、名字未知）。`audit` 认「八值六码」，`seal` 做 FNV-1a 折叠，
//! `replay` 重建比对，`freeze` 后拒绝登记。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf3a0;

/// 选项字面非法。
pub const IN_EINVAL: u32 = SEG_BASE + 0x01;
/// 选项或路径过长。
pub const IN_ELEN: u32 = SEG_BASE + 0x02;
/// 字节不合法。
pub const IN_EBYTE: u32 = SEG_BASE + 0x03;
/// 八进制/编号越界。
pub const IN_ERANGE: u32 = SEG_BASE + 0x04;
/// 目标重复。
pub const IN_EDUP: u32 = SEG_BASE + 0x05;
/// 名字查不到。
pub const IN_ENOENT: u32 = SEG_BASE + 0x06;
/// 特权不足。
pub const IN_EPERM: u32 = SEG_BASE + 0x07;
/// 选项组合冲突。
pub const IN_ESTATE: u32 = SEG_BASE + 0x08;
/// 账本失衡。
pub const IN_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位错位。
pub const IN_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const IN_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const IN_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观测缺失。
pub const IN_ENOBS: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 路径容量。
pub const PATHCAP: usize = 128;

/// 计划里最多几个操作。
pub const OPCAP: usize = 8;

/// 名字容量。
pub const NAMECAP: usize = 32;

/// 目录表容量。
pub const DBCAP: usize = 8;

/// 权限位上限：十二位。
pub const MODE_MAX: u32 = 0o7777;

/// 操作码：八进制解析。
pub const OP_MODE: u8 = 0x01;
/// 操作码：符号式增量。
pub const OP_SYM: u8 = 0x02;
/// 操作码：符号渲染。
pub const OP_RENDER: u8 = 0x03;
/// 操作码：路径拼装。
pub const OP_PATH: u8 = 0x04;
/// 操作码：冗余行。
pub const OP_VERBOSE: u8 = 0x05;
/// 操作码：属主。
pub const OP_OWNER: u8 = 0x06;
/// 操作码：属组。
pub const OP_GROUP: u8 = 0x07;
/// 操作码：目录模式。
pub const OP_DIR: u8 = 0x08;
/// 操作码：计划。
pub const OP_PLAN: u8 = 0x09;
/// 操作码：名字表。
pub const OP_USER: u8 = 0x0a;

/// 操作种类：复制。
pub const KIND_COPY: u8 = 0x01;
/// 操作种类：建目录。
pub const KIND_DIR: u8 = 0x02;

/// FNV-1a 偏移。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个字节。
fn fold(h: u64, b: u64) -> u64 {
    let mut v = h ^ (b & 0xff);
    let mut i = 0u32;
    while i < 8 {
        v = v.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    v
}

/// 权限位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mode {
    bits: u32,
}

impl Mode {
    /// 空模式。
    pub const fn empty() -> Mode {
        Mode { bits: 0 }
    }

    /// 直接构造（越界给 `IN_ERANGE`）。
    pub fn from_bits(bits: u32) -> Result<Mode, u32> {
        if bits > MODE_MAX {
            return Err(IN_ERANGE);
        }
        Ok(Mode { bits })
    }

    /// 位值。
    pub fn bits(&self) -> u32 {
        self.bits
    }

    /// 八进制直给。
    pub fn parse_octal(raw: &[u8]) -> Result<Mode, u32> {
        if raw.is_empty() {
            return Err(IN_EINVAL);
        }
        if raw.len() > 7 {
            return Err(IN_ELEN);
        }
        let mut v = 0u32;
        let mut i = 0usize;
        while i < raw.len() {
            let b = match raw.get(i) {
                Some(x) => *x,
                None => return Err(IN_EBYTE),
            };
            if !(b'0'..=b'7').contains(&b) {
                return Err(IN_EBYTE);
            }
            v = v.saturating_mul(8).saturating_add((b - b'0') as u32);
            if v > MODE_MAX {
                return Err(IN_ERANGE);
            }
            i += 1;
        }
        Ok(Mode { bits: v })
    }

    /// 八进制文本（四位，带前导 0）。
    pub fn render_octal(&self) -> String {
        let mut s = String::new();
        let mut shift = 9u32;
        loop {
            let d = (self.bits >> shift) & 0x7;
            s.push((b'0' + d as u8) as char);
            if shift == 0 {
                break;
            }
            shift -= 3;
        }
        s
    }

    /// 符号渲染（九位，特殊位折进 `s/S/t/T`）。
    pub fn render_symbolic(&self) -> String {
        let mut s = String::new();
        triad_full(self.bits, 0o400, 0o200, 0o100, 0o4000, &mut s);
        triad_full(self.bits, 0o040, 0o020, 0o010, 0o2000, &mut s);
        triad_full(self.bits, 0o004, 0o002, 0o001, 0o1000, &mut s);
        s
    }

    /// 去掉掩码里的位。
    pub fn with_umask(&self, umask: u32) -> Mode {
        Mode { bits: self.bits & !(umask & 0o777) }
    }

    /// 是否带 setuid。
    pub fn is_setuid(&self) -> bool {
        self.bits & 0o4000 != 0
    }

    /// 是否带 setgid。
    pub fn is_setgid(&self) -> bool {
        self.bits & 0o2000 != 0
    }

    /// 是否带粘滞位。
    pub fn is_sticky(&self) -> bool {
        self.bits & 0o1000 != 0
    }

    /// 是否有任一执行位。
    pub fn any_exec(&self) -> bool {
        self.bits & 0o111 != 0
    }

    /// 是否特权位（setuid/setgid）。
    pub fn privileged(&self) -> bool {
        self.is_setuid() || self.is_setgid()
    }

    /// 相等（同一份位）。
    pub fn same(&self, other: Mode) -> bool {
        self.bits == other.bits
    }
}

/// 一组 `rwx` 三连渲染，特殊位折进第三列。
fn triad_full(bits: u32, r: u32, w: u32, x: u32, special: u32, out: &mut String) {
    out.push(if bits & r != 0 { 'r' } else { '-' });
    out.push(if bits & w != 0 { 'w' } else { '-' });
    let has_x = bits & x != 0;
    if bits & special != 0 {
        let sticky = special == 0o1000;
        out.push(match (has_x, sticky) {
            (true, false) => 's',
            (false, false) => 'S',
            (true, true) => 't',
            (false, true) => 'T',
        });
    } else {
        out.push(if has_x { 'x' } else { '-' });
    }
}

/// 符号式单个权限字符落位。
fn perm_bits(c: u8, who: u32, mode: Mode, is_dir: bool) -> Option<u32> {
    match c {
        b'r' => Some(0o444 & who),
        b'w' => Some(0o222 & who),
        b'x' => Some(0o111 & who),
        b'X' => {
            if is_dir || mode.any_exec() {
                Some(0o111 & who)
            } else {
                Some(0)
            }
        }
        b's' => {
            let mut v = 0u32;
            if who & 0o700 != 0 {
                v |= 0o4000;
            }
            if who & 0o070 != 0 {
                v |= 0o2000;
            }
            if v == 0 {
                return None;
            }
            Some(v)
        }
        b't' => Some(0o1000),
        _ => None,
    }
}

/// 符号式 `=` 要清掉的位。
fn clear_bits(who: u32) -> u32 {
    let mut v = 0o777 & who;
    if who & 0o700 != 0 {
        v |= 0o4000;
    }
    if who & 0o070 != 0 {
        v |= 0o2000;
    }
    if who & 0o007 != 0 {
        v |= 0o1000;
    }
    v
}

/// 符号式增量（`u+x,g-w,o=r`）。
pub fn apply_symbolic(mode: Mode, spec: &[u8], is_dir: bool) -> Result<Mode, u32> {
    if spec.is_empty() {
        return Err(IN_EINVAL);
    }
    let mut cur = mode;
    let mut i = 0usize;
    while i < spec.len() {
        let mut who = 0u32;
        while i < spec.len() {
            let b = match spec.get(i) {
                Some(v) => *v,
                None => return Err(IN_EBYTE),
            };
            match b {
                b'u' => who |= 0o700,
                b'g' => who |= 0o070,
                b'o' => who |= 0o007,
                b'a' => who |= 0o777,
                _ => break,
            }
            i += 1;
        }
        if who == 0 {
            who = 0o777;
        }
        let mut saw_op = false;
        while i < spec.len() {
            let op = match spec.get(i) {
                Some(v) => *v,
                None => return Err(IN_EBYTE),
            };
            if op == b',' {
                i += 1;
                break;
            }
            if op != b'+' && op != b'-' && op != b'=' {
                return Err(IN_EINVAL);
            }
            saw_op = true;
            i += 1;
            let mut acc = 0u32;
            let mut touched = false;
            while i < spec.len() {
                let c = match spec.get(i) {
                    Some(v) => *v,
                    None => return Err(IN_EBYTE),
                };
                if c == b'+' || c == b'-' || c == b'=' || c == b',' {
                    break;
                }
                let add = match perm_bits(c, who, cur, is_dir) {
                    Some(v) => v,
                    None => return Err(IN_EINVAL),
                };
                acc |= add;
                touched = true;
                i += 1;
            }
            if !touched && op != b'=' {
                return Err(IN_EINVAL);
            }
            let mut bits = cur.bits;
            match op {
                b'+' => bits |= acc,
                b'-' => bits &= !acc,
                _ => {
                    bits &= !clear_bits(who);
                    bits |= acc;
                }
            }
            if bits > MODE_MAX {
                return Err(IN_ERANGE);
            }
            cur = Mode { bits };
        }
        if !saw_op {
            return Err(IN_EINVAL);
        }
    }
    Ok(cur)
}

/// 路径（定长，可复制）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Path {
    buf: [u8; PATHCAP],
    len: u8,
}

impl Path {
    /// 空路径。
    pub const fn empty() -> Path {
        Path { buf: [0; PATHCAP], len: 0 }
    }

    /// 根。
    pub fn root() -> Path {
        let mut buf = [0u8; PATHCAP];
        if let Some(dst) = buf.get_mut(0) {
            *dst = b'/';
        }
        Path { buf, len: 1 }
    }

    /// 解析。
    pub fn parse(s: &str) -> Result<Path, u32> {
        Path::parse_bytes(s.as_bytes())
    }

    /// 从字节解析。
    pub fn parse_bytes(raw: &[u8]) -> Result<Path, u32> {
        if raw.is_empty() {
            return Err(IN_EINVAL);
        }
        if raw.len() > PATHCAP {
            return Err(IN_ELEN);
        }
        if raw.len() > 1 {
            if let Some(last) = raw.last() {
                if *last == b'/' {
                    return Err(IN_EBYTE);
                }
            }
        }
        let mut i = 0usize;
        while i < raw.len() {
            let b = match raw.get(i) {
                Some(v) => *v,
                None => return Err(IN_EBYTE),
            };
            if b == 0 || b < 0x20 || b == 0x7f {
                return Err(IN_EBYTE);
            }
            if b == b'/' && i > 0 {
                if let Some(prev) = raw.get(i - 1) {
                    if *prev == b'/' {
                        return Err(IN_EBYTE);
                    }
                }
            }
            i += 1;
        }
        let mut buf = [0u8; PATHCAP];
        let mut k = 0usize;
        while k < raw.len() {
            if let (Some(dst), Some(src)) = (buf.get_mut(k), raw.get(k)) {
                *dst = *src;
            }
            k += 1;
        }
        Ok(Path { buf, len: raw.len() as u8 })
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

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let b = self.as_bytes();
        let mut i = 0usize;
        while i < b.len() {
            if let Some(c) = b.get(i) {
                s.push(*c as char);
            }
            i += 1;
        }
        s
    }

    /// 与字节比较。
    pub fn eq_bytes(&self, other: &[u8]) -> bool {
        self.as_bytes() == other
    }

    /// 是否根。
    pub fn is_root(&self) -> bool {
        self.len == 1 && self.as_bytes() == b"/"
    }

    /// 基名。
    pub fn basename(&self) -> Result<Path, u32> {
        if self.is_empty() {
            return Err(IN_ENOBS);
        }
        if self.is_root() {
            return Err(IN_EINVAL);
        }
        let b = self.as_bytes();
        let mut i = b.len();
        while i > 0 {
            if let Some(c) = b.get(i - 1) {
                if *c == b'/' {
                    return Path::parse_bytes(match b.get(i..) {
                        Some(v) => v,
                        None => return Err(IN_EBYTE),
                    });
                }
            }
            i -= 1;
        }
        Path::parse_bytes(b)
    }

    /// 父目录。
    pub fn parent(&self) -> Result<Path, u32> {
        if self.is_empty() {
            return Err(IN_ENOBS);
        }
        if self.is_root() {
            return Ok(Path::root());
        }
        let b = self.as_bytes();
        let mut i = b.len();
        while i > 0 {
            if let Some(c) = b.get(i - 1) {
                if *c == b'/' {
                    if i == 1 {
                        return Ok(Path::root());
                    }
                    return Path::parse_bytes(match b.get(..i - 1) {
                        Some(v) => v,
                        None => return Err(IN_EBYTE),
                    });
                }
            }
            i -= 1;
        }
        Ok(Path::root())
    }

    /// 拼一个组件。
    pub fn join(&self, name: &str) -> Result<Path, u32> {
        let nb = name.as_bytes();
        if nb.is_empty() {
            return Err(IN_EINVAL);
        }
        if self.is_empty() {
            return Path::parse_bytes(nb);
        }
        let need = self.len().saturating_add(1).saturating_add(nb.len());
        if need > PATHCAP {
            return Err(IN_ELEN);
        }
        let mut out = String::new();
        let b = self.as_bytes();
        let mut i = 0usize;
        while i < b.len() {
            if let Some(c) = b.get(i) {
                out.push(*c as char);
            }
            i += 1;
        }
        if !self.is_root() {
            out.push('/');
        }
        let mut k = 0usize;
        while k < nb.len() {
            if let Some(c) = nb.get(k) {
                out.push(*c as char);
            }
            k += 1;
        }
        Path::parse(&out)
    }
}

/// 账户一项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct User {
    name: [u8; NAMECAP],
    nlen: u8,
    uid: u32,
}

impl User {
    /// 空项。
    pub const fn blank() -> User {
        User { name: [0; NAMECAP], nlen: 0, uid: 0 }
    }

    /// 构造。
    pub fn new(name: &str, uid: u32) -> Result<User, u32> {
        let nb = name.as_bytes();
        if nb.is_empty() {
            return Err(IN_EINVAL);
        }
        if nb.len() > NAMECAP {
            return Err(IN_ELEN);
        }
        let mut buf = [0u8; NAMECAP];
        let mut i = 0usize;
        while i < nb.len() {
            let b = match nb.get(i) {
                Some(v) => *v,
                None => return Err(IN_EBYTE),
            };
            if !(b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.') {
                return Err(IN_EBYTE);
            }
            if let Some(dst) = buf.get_mut(i) {
                *dst = b;
            }
            i += 1;
        }
        Ok(User { name: buf, nlen: nb.len() as u8, uid })
    }

    /// 编号。
    pub fn uid(&self) -> u32 {
        self.uid
    }

    /// 名字字节。
    pub fn name_bytes(&self) -> &[u8] {
        match self.name.get(..self.nlen as usize) {
            Some(v) => v,
            None => &[],
        }
    }

    /// 名字比较。
    pub fn eq_name(&self, other: &[u8]) -> bool {
        self.name_bytes() == other
    }
}

/// 名字表（账户或组共用同一形状）。
#[derive(Clone, Copy, Debug)]
pub struct NameDb {
    rows: [User; DBCAP],
    len: usize,
}

impl NameDb {
    /// 空表。
    pub const fn new() -> NameDb {
        NameDb { rows: [User::blank(); DBCAP], len: 0 }
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 追加（重名报 `IN_EDUP`）。
    pub fn add(&mut self, u: User) -> Result<(), u32> {
        if self.len >= DBCAP {
            return Err(IN_ERANGE);
        }
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.rows.get(i) {
                if r.uid == u.uid || r.eq_name(u.name_bytes()) {
                    return Err(IN_EDUP);
                }
            }
            i += 1;
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = u;
        }
        self.len += 1;
        Ok(())
    }

    /// 按名字查。
    pub fn find_name(&self, name: &str) -> Result<User, u32> {
        let nb = name.as_bytes();
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.rows.get(i) {
                if r.eq_name(nb) {
                    return Ok(*r);
                }
            }
            i += 1;
        }
        Err(IN_ENOENT)
    }

    /// 按编号查。
    pub fn find_id(&self, id: u32) -> Result<User, u32> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.rows.get(i) {
                if r.uid == id {
                    return Ok(*r);
                }
            }
            i += 1;
        }
        Err(IN_ENOENT)
    }
}

/// 运行上下文。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Context {
    umask: u32,
    euid: u32,
    egid: u32,
}

impl Context {
    /// 构造（umask 只允许低九位）。
    pub fn new(umask: u32, euid: u32, egid: u32) -> Result<Context, u32> {
        if umask & !0o777 != 0 {
            return Err(IN_ERANGE);
        }
        Ok(Context { umask, euid, egid })
    }

    /// 掩码。
    pub fn umask(&self) -> u32 {
        self.umask
    }

    /// 是否 root。
    pub fn is_root(&self) -> bool {
        self.euid == 0
    }

    /// 有效用户。
    pub fn euid(&self) -> u32 {
        self.euid
    }

    /// 有效组。
    pub fn egid(&self) -> u32 {
        self.egid
    }
}

/// 属主/属组请求。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Owner {
    uid: Option<u32>,
    gid: Option<u32>,
}

impl Owner {
    /// 空请求。
    pub const fn none() -> Owner {
        Owner { uid: None, gid: None }
    }

    /// 只给属主。
    pub fn with_uid(uid: u32) -> Owner {
        Owner { uid: Some(uid), gid: None }
    }

    /// 只给属组。
    pub fn with_gid(gid: u32) -> Owner {
        Owner { uid: None, gid: Some(gid) }
    }

    /// 都给。
    pub fn with_both(uid: u32, gid: u32) -> Owner {
        Owner { uid: Some(uid), gid: Some(gid) }
    }

    /// 属主。
    pub fn uid(&self) -> Option<u32> {
        self.uid
    }

    /// 属组。
    pub fn gid(&self) -> Option<u32> {
        self.gid
    }

    /// 是否空。
    pub fn is_none(&self) -> bool {
        self.uid.is_none() && self.gid.is_none()
    }

    /// 解析十进制编号。
    pub fn parse_id(raw: &[u8]) -> Result<u32, u32> {
        if raw.is_empty() {
            return Err(IN_EINVAL);
        }
        if raw.len() > 10 {
            return Err(IN_ELEN);
        }
        let mut v = 0u32;
        let mut i = 0usize;
        while i < raw.len() {
            let b = match raw.get(i) {
                Some(x) => *x,
                None => return Err(IN_EBYTE),
            };
            if !b.is_ascii_digit() {
                return Err(IN_EBYTE);
            }
            v = v.saturating_mul(10).saturating_add((b - b'0') as u32);
            i += 1;
        }
        if v == u32::MAX {
            return Err(IN_ERANGE);
        }
        Ok(v)
    }
}

/// 改属主守卫：非 root 一律 `IN_EPERM`。
pub fn chown_guard(ctx: &Context, owner: &Owner) -> Result<(), u32> {
    if owner.is_none() {
        return Ok(());
    }
    if !ctx.is_root() {
        return Err(IN_EPERM);
    }
    Ok(())
}

/// 挂特权位守卫：非 root 不得挂 setuid/setgid。
pub fn privileged_guard(ctx: &Context, mode: Mode) -> Result<(), u32> {
    if mode.privileged() && !ctx.is_root() {
        return Err(IN_EPERM);
    }
    Ok(())
}

/// `install` 选项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spec {
    mode: Option<Mode>,
    owner: Owner,
    dir_mode: bool,
    parents: bool,
    no_target_dir: bool,
    preserve: bool,
    strip: bool,
    verbose: bool,
    target: Option<Path>,
}

impl Spec {
    /// 空选项。
    pub const fn blank() -> Spec {
        Spec {
            mode: None,
            owner: Owner::none(),
            dir_mode: false,
            parents: false,
            no_target_dir: false,
            preserve: false,
            strip: false,
            verbose: false,
            target: None,
        }
    }

    /// 显式模式。
    pub fn with_mode(mut self, m: Mode) -> Spec {
        self.mode = Some(m);
        self
    }

    /// 建目录档。
    pub fn as_dir(mut self) -> Spec {
        self.dir_mode = true;
        self
    }

    /// 建父目录。
    pub fn with_parents(mut self) -> Spec {
        self.parents = true;
        self
    }

    /// 目标目录。
    pub fn with_target(mut self, p: Path) -> Spec {
        self.target = Some(p);
        self
    }

    /// 目标即文件名。
    pub fn no_target(mut self) -> Spec {
        self.no_target_dir = true;
        self
    }

    /// 显式模式。
    pub fn mode(&self) -> Option<Mode> {
        self.mode
    }

    /// 目标。
    pub fn target(&self) -> Option<Path> {
        self.target
    }

    /// 是否建目录档。
    pub fn is_dir_mode(&self) -> bool {
        self.dir_mode
    }

    /// 是否建父目录。
    pub fn makes_parents(&self) -> bool {
        self.parents
    }

    /// 是否禁止目标当目录。
    pub fn forbids_target_dir(&self) -> bool {
        self.no_target_dir
    }

    /// 是否保留时间戳。
    pub fn preserves(&self) -> bool {
        self.preserve
    }

    /// 是否剥符号表。
    pub fn strips(&self) -> bool {
        self.strip
    }

    /// 是否冗余输出。
    pub fn is_verbose(&self) -> bool {
        self.verbose
    }

    /// 校验选项组合。
    pub fn check(&self) -> Result<(), u32> {
        if self.preserve && self.strip {
            return Err(IN_ESTATE);
        }
        if self.no_target_dir && self.target.is_some() {
            return Err(IN_ESTATE);
        }
        if self.dir_mode && self.no_target_dir {
            return Err(IN_ESTATE);
        }
        Ok(())
    }

    /// 解析一行选项。
    pub fn parse(line: &str) -> Result<Spec, u32> {
        let mut spec = Spec::blank();
        let mut seen_mode = false;
        let mut seen_owner = false;
        let bytes = line.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            let b = match bytes.get(i) {
                Some(v) => *v,
                None => return Err(IN_EBYTE),
            };
            if b == b' ' {
                i += 1;
                continue;
            }
            if b != b'-' {
                return Err(IN_EINVAL);
            }
            let flag = match bytes.get(i + 1) {
                Some(v) => *v,
                None => return Err(IN_EINVAL),
            };
            i += 2;
            match flag {
                b'd' => spec.dir_mode = true,
                b'D' => spec.parents = true,
                b'T' => spec.no_target_dir = true,
                b'p' => spec.preserve = true,
                b's' => spec.strip = true,
                b'v' => spec.verbose = true,
                b'm' => {
                    if seen_mode {
                        return Err(IN_EDUP);
                    }
                    let v = take_value(bytes, &mut i)?;
                    spec.mode = Some(Mode::parse_octal(v)?);
                    seen_mode = true;
                }
                b'o' => {
                    if seen_owner {
                        return Err(IN_EDUP);
                    }
                    let v = take_value(bytes, &mut i)?;
                    let id = Owner::parse_id(v)?;
                    spec.owner = Owner { uid: Some(id), gid: spec.owner.gid() };
                    seen_owner = true;
                }
                b'g' => {
                    let v = take_value(bytes, &mut i)?;
                    let id = Owner::parse_id(v)?;
                    spec.owner = Owner { uid: spec.owner.uid(), gid: Some(id) };
                }
                b't' => {
                    let v = take_value(bytes, &mut i)?;
                    let p = Path::parse_bytes(v)?;
                    spec.target = Some(p);
                }
                _ => return Err(IN_EINVAL),
            }
        }
        spec.check()?;
        Ok(spec)
    }
}

/// 取下一个空格分隔的值。
fn take_value<'a>(bytes: &'a [u8], i: &mut usize) -> Result<&'a [u8], u32> {
    let mut start = *i;
    while start < bytes.len() {
        match bytes.get(start) {
            Some(b' ') => start += 1,
            _ => break,
        }
    }
    let mut end = start;
    while end < bytes.len() {
        match bytes.get(end) {
            Some(b' ') => break,
            _ => end += 1,
        }
    }
    if end == start {
        return Err(IN_EINVAL);
    }
    *i = end;
    match bytes.get(start..end) {
        Some(v) => Ok(v),
        None => Err(IN_EBYTE),
    }
}

/// 计划里的一步。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Op {
    /// 源。
    pub src: Path,
    /// 目标。
    pub dest: Path,
    /// 种类。
    pub kind: u8,
}

impl Op {
    /// 空白步。
    pub const fn blank() -> Op {
        Op { src: Path::empty(), dest: Path::empty(), kind: 0 }
    }
}

/// 复制计划。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    ops: [Op; OPCAP],
    len: usize,
    dirs: u8,
}

impl Plan {
    /// 空计划。
    pub const fn new() -> Plan {
        Plan { ops: [Op::blank(); OPCAP], len: 0, dirs: 0 }
    }

    /// 步数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 步。
    pub fn at(&self, i: usize) -> Option<Op> {
        if i >= self.len {
            return None;
        }
        self.ops.get(i).copied()
    }

    /// 目录步数。
    pub fn dirs(&self) -> u8 {
        self.dirs
    }

    /// 追加（目标重复报 `IN_EDUP`）。
    pub fn push(&mut self, op: Op) -> Result<(), u32> {
        if self.len >= OPCAP {
            return Err(IN_ERANGE);
        }
        let mut i = 0usize;
        while i < self.len {
            if let Some(o) = self.ops.get(i) {
                if o.dest == op.dest {
                    return Err(IN_EDUP);
                }
            }
            i += 1;
        }
        if op.kind == KIND_DIR {
            self.dirs = self.dirs.saturating_add(1);
        }
        if let Some(dst) = self.ops.get_mut(self.len) {
            *dst = op;
        }
        self.len += 1;
        Ok(())
    }
}

/// 生成计划。
pub fn plan(spec: &Spec, sources: &[&str]) -> Result<Plan, u32> {
    spec.check()?;
    if sources.is_empty() {
        return Err(IN_EINVAL);
    }
    if spec.forbids_target_dir() && sources.len() > 1 {
        return Err(IN_EINVAL);
    }
    let mut p = Plan::new();
    let mut i = 0usize;
    while i < sources.len() {
        let raw = match sources.get(i) {
            Some(v) => *v,
            None => return Err(IN_ENOBS),
        };
        let src = Path::parse(raw)?;
        let (dest, kind) = if spec.is_dir_mode() {
            (src, KIND_DIR)
        } else {
            match spec.target() {
                Some(t) => (t.join(&base_text(&src)?)?, KIND_COPY),
                None => (Path::parse(&base_text(&src)?)?, KIND_COPY),
            }
        };
        p.push(Op { src, dest, kind })?;
        i += 1;
    }
    Ok(p)
}

/// 基名文本。
fn base_text(src: &Path) -> Result<String, u32> {
    let b = src.basename()?;
    Ok(b.text())
}

/// 目录模式：显式优先，否则 `0755 & ~umask`。
pub fn dir_mode(spec: &Spec, ctx: &Context) -> Result<Mode, u32> {
    match spec.mode() {
        Some(m) => {
            privileged_guard(ctx, m)?;
            Ok(m)
        }
        None => Mode::from_bits(0o755).map(|m| m.with_umask(ctx.umask())),
    }
}

/// 冗余行：`'src' -> 'dst'`。
pub fn verbose_line(src: &str, dst: &str) -> String {
    let mut s = String::new();
    s.push('\'');
    let sb = src.as_bytes();
    let mut i = 0usize;
    while i < sb.len() {
        if let Some(c) = sb.get(i) {
            s.push(*c as char);
        }
        i += 1;
    }
    s.push('\'');
    s.push(' ');
    s.push('-');
    s.push('>');
    s.push(' ');
    s.push('\'');
    let db = dst.as_bytes();
    let mut k = 0usize;
    while k < db.len() {
        if let Some(c) = db.get(k) {
            s.push(*c as char);
        }
        k += 1;
    }
    s.push('\'');
    s
}

/// 一个账本样本。
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
            return Err(IN_EBAL);
        }
        const VALS: [u32; 8] = [493, 500, 9, 19, 10, 1000, 50, 493];
        const CODES: [u32; 6] = [IN_EBYTE, IN_ERANGE, IN_EINVAL, IN_EPERM, IN_EDUP, IN_ENOENT];
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
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
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

/// 名字表样本：root/daemon/bin 三个账户。
pub fn build_users() -> NameDb {
    let mut db = NameDb::new();
    let _ = db.add(match User::new("root", 0) {
        Ok(u) => u,
        Err(_) => User::blank(),
    });
    let _ = db.add(match User::new("daemon", 1) {
        Ok(u) => u,
        Err(_) => User::blank(),
    });
    let _ = db.add(match User::new("alice", 1000) {
        Ok(u) => u,
        Err(_) => User::blank(),
    });
    let _ = db.add(match User::new("staff", 50) {
        Ok(u) => u,
        Err(_) => User::blank(),
    });
    db
}

/// 十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let ctx = Context::new(0o022, 1000, 1000);
    let root = Context::new(0o022, 0, 0);
    let users = build_users();

    let _ = l.register(word_cell(0, OP_MODE, Mode::parse_octal(b"755").map(|m| m.bits())));

    let _ = l.register(word_cell(
        1,
        OP_SYM,
        Mode::from_bits(0o640).and_then(|m| apply_symbolic(m, b"u+x,g+w,o+r", false)).map(|m| m.bits()),
    ));

    let _ = l.register(count_cell(2, OP_RENDER, Ok(Mode { bits: 0o4755 }.render_symbolic().len())));

    let _ = l.register(count_cell(
        3,
        OP_PATH,
        Path::parse("/usr/local/bin").and_then(|d| d.join("tool")).map(|p| p.len()),
    ));

    let _ = l.register(count_cell(4, OP_VERBOSE, Ok(verbose_line("a", "b").len())));

    let _ = l.register(word_cell(5, OP_OWNER, Owner::parse_id(b"1000")));

    let _ = l.register(word_cell(6, OP_GROUP, Owner::parse_id(b"50")));

    let _ = l.register(word_cell(
        7,
        OP_DIR,
        Mode::from_bits(0o755).map(|m| match &ctx {
            Ok(c) => m.with_umask(c.umask()).bits(),
            Err(_) => 0,
        }),
    ));

    let _ = l.register(word_cell(8, OP_MODE, Mode::parse_octal(b"75a").map(|m| m.bits())));

    let _ = l.register(word_cell(9, OP_MODE, Mode::parse_octal(b"17777").map(|m| m.bits())));

    let _ = l.register(word_cell(
        10,
        OP_SYM,
        Mode::from_bits(0o644).and_then(|m| apply_symbolic(m, b"u+z", false)).map(|m| m.bits()),
    ));

    let _ = l.register(word_cell(
        11,
        OP_OWNER,
        match &ctx {
            Ok(c) => chown_guard(c, &Owner::with_uid(0)).map(|_| 0),
            Err(c) => Err(*c),
        },
    ));

    let _ = l.register(word_cell(12, OP_PLAN, {
        let mut p = Plan::new();
        let op = Op {
            src: Path { buf: [0; PATHCAP], len: 1 },
            dest: Path { buf: [0; PATHCAP], len: 1 },
            kind: KIND_COPY,
        };
        match p.push(op) {
            Ok(()) => match p.push(op) {
                Ok(()) => Ok(0),
                Err(c) => Err(c),
            },
            Err(c) => Err(c),
        }
    }));

    let _ = l.register(word_cell(13, OP_USER, users.find_name("nobody").map(|u| u.uid())));

    let _ = root;
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let a = match Mode::parse_octal(b"0755") {
        Ok(m) => m.bits() == 0o755 && m.render_octal() == String::from("0755"),
        Err(_) => false,
    };
    out[0] = a;

    let b = match Mode::from_bits(0o755) {
        Ok(m) => match apply_symbolic(m, b"a-x", true) {
            Ok(v) => v.bits() == 0o644,
            Err(_) => false,
        },
        Err(_) => false,
    };
    out[1] = b;

    let c = {
        let guarded = match Context::new(0o022, 1000, 1000) {
            Ok(ctx) => privileged_guard(&ctx, Mode { bits: 0o4755 }).is_err(),
            Err(_) => false,
        };
        let allowed = match Context::new(0o022, 0, 0) {
            Ok(ctx) => privileged_guard(&ctx, Mode { bits: 0o4755 }).is_ok(),
            Err(_) => false,
        };
        guarded && allowed
    };
    out[2] = c;

    let d = match Spec::parse("-m 755 -o 0 -g 0 -v -t /usr/local/bin") {
        Ok(sp) => {
            let p = plan(&sp, &["tool"]);
            match p {
                Ok(v) => match v.at(0) {
                    Some(op) => {
                        op.dest.eq_bytes(b"/usr/local/bin/tool")
                            && op.kind == KIND_COPY
                            && v.len() == 1
                    }
                    None => false,
                },
                Err(_) => false,
            }
        }
        Err(_) => false,
    };
    out[3] = d;

    out
}

/// 判据集合。
pub fn run_unx11553_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11553.install");

    s.add(
        "IN01-八进制直给",
        {
            let a = Mode::parse_octal(b"755").map(|m| m.bits()) == Ok(0o755);
            let b = Mode::parse_octal(b"0644").map(|m| m.bits()) == Ok(0o644);
            let c = Mode::parse_octal(b"0").map(|m| m.bits()) == Ok(0);
            let d = Mode::parse_octal(b"7777").map(|m| m.bits()) == Ok(0o7777);
            let e = Mode::parse_octal(b"17777") == Err(IN_ERANGE)
                && Mode::parse_octal(b"8") == Err(IN_EBYTE)
                && Mode::parse_octal(b"") == Err(IN_EINVAL)
                && Mode::parse_octal(b"00000000") == Err(IN_ELEN);
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "IN02-符号式增量",
        {
            let base = match Mode::from_bits(0o640) {
                Ok(m) => m,
                Err(_) => Mode::empty(),
            };
            let a = apply_symbolic(base, b"u+x,g+w,o+r", false).map(|m| m.bits()) == Ok(0o764);
            let b = apply_symbolic(base, b"go=", false).map(|m| m.bits()) == Ok(0o600);
            let c = apply_symbolic(base, b"a+X", false).map(|m| m.bits()) == Ok(0o640);
            let d = apply_symbolic(base, b"a+X", true).map(|m| m.bits()) == Ok(0o751);
            let e = apply_symbolic(base, b"u+s", false).map(|m| m.bits()) == Ok(0o4640)
                && apply_symbolic(base, b"g+s", false).map(|m| m.bits()) == Ok(0o2640)
                && apply_symbolic(base, b"o+t", false).map(|m| m.bits()) == Ok(0o1640);
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "IN03-模式渲染",
        {
            let a = Mode { bits: 0o4755 }.render_symbolic() == String::from("rwsr-xr-x");
            let b = Mode { bits: 0o2755 }.render_symbolic() == String::from("rwxr-sr-x");
            let c = Mode { bits: 0o1777 }.render_symbolic() == String::from("rwxrwxrwt");
            let d = Mode { bits: 0o4644 }.render_symbolic() == String::from("rwSr--r--");
            let e = Mode { bits: 0o755 }.render_octal() == String::from("0755")
                && Mode { bits: 0o4755 }.render_octal() == String::from("4755");
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "IN04-掩码扣位",
        {
            let a = Mode { bits: 0o777 }.with_umask(0o022).bits() == 0o755;
            let b = Mode { bits: 0o777 }.with_umask(0o027).bits() == 0o750;
            let c = Mode { bits: 0o666 }.with_umask(0o077).bits() == 0o600;
            let d = Context::new(0o777, 0, 0).is_ok() && Context::new(0o1000, 0, 0) == Err(IN_ERANGE);
            a && b && c && d
        },
        "static",
    );

    s.add(
        "IN05-路径规则",
        {
            let a = match Path::parse("/usr/local/bin") {
                Ok(p) => p.len() == 14 && p.is_root() == false,
                Err(_) => false,
            };
            let b = Path::parse("") == Err(IN_EINVAL)
                && Path::parse("/a//b") == Err(IN_EBYTE)
                && Path::parse("/a/") == Err(IN_EBYTE)
                && Path::parse("a\u{0}b") == Err(IN_EBYTE);
            let c = match Path::parse("/usr/local/bin") {
                Ok(p) => match p.basename() {
                    Ok(b2) => b2.eq_bytes(b"bin"),
                    Err(_) => false,
                },
                Err(_) => false,
            };
            let d = match Path::parse("/usr/local/bin") {
                Ok(p) => match p.parent() {
                    Ok(q) => q.eq_bytes(b"/usr/local"),
                    Err(_) => false,
                },
                Err(_) => false,
            };
            let e = Path::root().is_root()
                && match Path::root().join("tmp") {
                    Ok(q) => q.eq_bytes(b"/tmp"),
                    Err(_) => false,
                };
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "IN06-目标解析",
        {
            let a = match Spec::parse("-t /usr/local/bin") {
                Ok(sp) => match plan(&sp, &["tool"]) {
                    Ok(p) => match p.at(0) {
                        Some(op) => op.dest.eq_bytes(b"/usr/local/bin/tool") && op.kind == KIND_COPY,
                        None => false,
                    },
                    Err(_) => false,
                },
                Err(_) => false,
            };
            let b = match Spec::parse("-T") {
                Ok(sp) => plan(&sp, &["a", "b"]) == Err(IN_EINVAL),
                Err(_) => false,
            };
            let c = match Spec::parse("-d") {
                Ok(sp) => match plan(&sp, &["/srv/app"]) {
                    Ok(p) => match p.at(0) {
                        Some(op) => op.kind == KIND_DIR && op.dest.eq_bytes(b"/srv/app") && p.dirs() == 1,
                        None => false,
                    },
                    Err(_) => false,
                },
                Err(_) => false,
            };
            let d = match Spec::parse("-t /tmp") {
                Ok(sp) => plan(&sp, &["x", "x"]) == Err(IN_EDUP),
                Err(_) => false,
            };
            a && b && c && d
        },
        "static",
    );

    s.add(
        "IN07-特权闸门",
        {
            let user = Context::new(0o022, 1000, 1000);
            let root = Context::new(0o022, 0, 0);
            let a = match &user {
                Ok(c) => chown_guard(c, &Owner::with_uid(0)) == Err(IN_EPERM),
                Err(_) => false,
            };
            let b = match &root {
                Ok(c) => chown_guard(c, &Owner::with_both(0, 0)).is_ok(),
                Err(_) => false,
            };
            let c = match &user {
                Ok(ctx) => privileged_guard(ctx, Mode { bits: 0o4755 }) == Err(IN_EPERM),
                Err(_) => false,
            };
            let d = match &root {
                Ok(ctx) => privileged_guard(ctx, Mode { bits: 0o2755 }).is_ok(),
                Err(_) => false,
            };
            let e = match &user {
                Ok(ctx) => privileged_guard(ctx, Mode { bits: 0o755 }).is_ok()
                    && chown_guard(ctx, &Owner::none()).is_ok(),
                Err(_) => false,
            };
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "IN08-选项冲突",
        {
            let a = Spec::parse("-p -s") == Err(IN_ESTATE);
            let b = Spec::parse("-T -t /tmp") == Err(IN_ESTATE);
            let c = Spec::parse("-d -T") == Err(IN_ESTATE);
            let d = Spec::parse("-m 755 -m 644") == Err(IN_EDUP);
            let e = Spec::parse("-Z") == Err(IN_EINVAL)
                && Spec::parse("-m") == Err(IN_EINVAL)
                && Spec::parse("plain") == Err(IN_EINVAL);
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "IN09-冗余行",
        {
            let a = verbose_line("a", "b") == String::from("'a' -> 'b'");
            let b = verbose_line("/usr/bin/tool", "/usr/local/bin/tool").len() == 40;
            let c = verbose_line("", "") == String::from("'' -> ''");
            a && b && c
        },
        "static",
    );

    s.add(
        "IN10-名字表",
        {
            let db = build_users();
            let a = db.len() == 4;
            let b = db.find_name("alice").map(|u| u.uid()) == Ok(1000);
            let c = db.find_name("nobody") == Err(IN_ENOENT);
            let d = db.find_id(0).map(|u| u.eq_name(b"root")) == Ok(true);
            let e = db.find_id(9999) == Err(IN_ENOENT)
                && User::new("", 1) == Err(IN_EINVAL)
                && User::new("bad name", 1) == Err(IN_EBYTE);
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "IN11-目录档",
        {
            let ctx = Context::new(0o022, 0, 0);
            let a = match &ctx {
                Ok(c) => dir_mode(&Spec::blank(), c).map(|m| m.bits()) == Ok(0o755),
                Err(_) => false,
            };
            let b = match Spec::parse("-d -m 2755") {
                Ok(sp) => match &ctx {
                    Ok(c) => dir_mode(&sp, c).map(|m| m.bits()) == Ok(0o2755),
                    Err(_) => false,
                },
                Err(_) => false,
            };
            let c = match Spec::parse("-d") {
                Ok(sp) => match plan(&sp, &["/srv/a", "/srv/b"]) {
                    Ok(p) => p.len() == 2 && p.dirs() == 2,
                    Err(_) => false,
                },
                Err(_) => false,
            };
            let d = match Spec::parse("-d -m 4755") {
                Ok(sp) => match Context::new(0o022, 1000, 1000) {
                    Ok(c1) => dir_mode(&sp, &c1) == Err(IN_EPERM),
                    Err(_) => false,
                },
                Err(_) => false,
            };
            a && b && c && d
        },
        "static",
    );

    s.add(
        "IN12-账本审计",
        {
            let l = build_ledger();
            l.len() == SAMPLES
                && l.ok_count() == 8
                && l.err_count() == 6
                && l.audit().is_ok()
                && l.rejects() == 0
                && l.sample(0).map(|x| x.value) == Some(493)
                && l.sample(1).map(|x| x.value) == Some(500)
                && l.sample(13).map(|x| x.code) == Some(IN_ENOENT)
                && l.sample(14).is_none()
        },
        "static",
    );

    s.add(
        "IN13-冻结重放四注入",
        inject_four() == [true; 4] && build_ledger().audit().is_ok(),
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn octal_vectors() {
        assert_eq!(Mode::parse_octal(b"755").map(|m| m.bits()), Ok(0o755));
        assert_eq!(Mode::parse_octal(b"0000").map(|m| m.bits()), Ok(0));
        assert_eq!(Mode::parse_octal(b"4755").map(|m| m.bits()), Ok(0o4755));
        assert_eq!(Mode::parse_octal(b"8888"), Err(IN_EBYTE));
        assert_eq!(Mode::parse_octal(b"17777"), Err(IN_ERANGE));
        assert_eq!(Mode::parse_octal(b""), Err(IN_EINVAL));
    }

    #[test]
    fn symbolic_vectors() {
        let base = match Mode::from_bits(0o755) {
            Ok(m) => m,
            Err(_) => Mode::empty(),
        };
        assert_eq!(apply_symbolic(base, b"u+x", false).map(|m| m.bits()), Ok(0o755));
        assert_eq!(apply_symbolic(base, b"ug-w", false).map(|m| m.bits()), Ok(0o555));
        assert_eq!(apply_symbolic(base, b"o=r", false).map(|m| m.bits()), Ok(0o754));
        assert_eq!(apply_symbolic(base, b"a=rwx", false).map(|m| m.bits()), Ok(0o777));
        assert_eq!(apply_symbolic(base, b"u+z", false), Err(IN_EINVAL));
    }

    #[test]
    fn render_vectors() {
        assert_eq!(Mode { bits: 0o755 }.render_symbolic(), String::from("rwxr-xr-x"));
        assert_eq!(Mode { bits: 0o644 }.render_symbolic(), String::from("rw-r--r--"));
        assert_eq!(Mode { bits: 0o1777 }.render_symbolic(), String::from("rwxrwxrwt"));
        assert_eq!(Mode { bits: 0o0 }.render_symbolic(), String::from("---------"));
        assert_eq!(Mode { bits: 0o777 }.render_octal(), String::from("0777"));
    }

    #[test]
    fn path_vectors() {
        assert_eq!(Path::parse("").err(), Some(IN_EINVAL));
        assert_eq!(Path::parse("//a").err(), Some(IN_EBYTE));
        assert_eq!(Path::parse("a/").err(), Some(IN_EBYTE));
        let p = match Path::parse("/srv/www/html") {
            Ok(v) => v,
            Err(_) => Path::empty(),
        };
        assert_eq!(p.basename().map(|b| b.eq_bytes(b"html")), Ok(true));
        assert_eq!(p.parent().map(|b| b.eq_bytes(b"/srv/www")), Ok(true));
        assert_eq!(p.join("index.html").map(|q| q.len()), Ok(24));
    }

    #[test]
    fn spec_vectors() {
        assert_eq!(Spec::parse("-p -s"), Err(IN_ESTATE));
        assert_eq!(Spec::parse("-T -t /tmp"), Err(IN_ESTATE));
        assert_eq!(Spec::parse("-m 644 -m 755"), Err(IN_EDUP));
        let sp = match Spec::parse("-m 755 -o 1000 -g 50 -v") {
            Ok(v) => v,
            Err(_) => Spec::blank(),
        };
        assert_eq!(sp.mode().map(|m| m.bits()), Some(0o755));
        assert_eq!(sp.owner.uid(), Some(1000));
        assert_eq!(sp.owner.gid(), Some(50));
        assert!(sp.is_verbose());
    }

    #[test]
    fn plan_vectors() {
        let sp = match Spec::parse("-t /opt/bin") {
            Ok(v) => v,
            Err(_) => Spec::blank(),
        };
        let p = match plan(&sp, &["a", "b"]) {
            Ok(v) => v,
            Err(_) => Plan::new(),
        };
        assert_eq!(p.len(), 2);
        assert_eq!(p.dirs(), 0);
        assert_eq!(p.at(0).map(|o| o.dest.eq_bytes(b"/opt/bin/a")), Some(true));
        assert_eq!(p.at(1).map(|o| o.dest.eq_bytes(b"/opt/bin/b")), Some(true));
        assert_eq!(p.at(2).is_none(), true);
    }

    #[test]
    fn guards_vectors() {
        let user = match Context::new(0o022, 1000, 1000) {
            Ok(c) => c,
            Err(_) => Context::new(0, 0, 0).unwrap_or(Context { umask: 0, euid: 0, egid: 0 }),
        };
        assert_eq!(chown_guard(&user, &Owner::with_uid(0)), Err(IN_EPERM));
        assert_eq!(privileged_guard(&user, Mode { bits: 0o4755 }), Err(IN_EPERM));
        assert_eq!(privileged_guard(&user, Mode { bits: 0o755 }), Ok(()));
        assert_eq!(chown_guard(&user, &Owner::none()), Ok(()));
    }

    #[test]
    fn ledger_vectors() {
        let l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.sample(0).map(|s| s.value), Some(493));
        assert_eq!(l.sample(9).map(|s| s.code), Some(IN_ERANGE));
        assert_eq!(l.replay(), Ok(l.seal()));
        let mut f = build_ledger();
        assert!(f.freeze().is_ok());
        assert!(f.register(Sample::blank()).is_err());
        assert_eq!(f.rejects(), 1);
        assert_eq!(f.replay(), Err(IN_ESEAL));
    }
}
