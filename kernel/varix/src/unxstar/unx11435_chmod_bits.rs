//! UNX-F11435 chmod 权限位（0x20af0 段）
//!
//! chmod 接受两种语法：三到四位的八进制数与逗号分隔的符号子句。模型把两种语法
//! 都归约成对 12 个模式位的读写：三个特殊位（setuid、setgid、sticky）和九个
//! rwx 位。八进制路径要求每位不超过 7、总值不超过 07777；符号路径逐个解析
//! who（u/g/o/a，缺省视为 a）、操作符（+/-/=）与权限字母（rwxXst 以及拷贝源
//! u/g/o）。任何空子句、尾随逗号、未知字母都返回 EINVAL，且不产生任何修改。
//!
//! 语义上真正复杂的不是置位而是清除。`=` 先清空 who 所覆盖的普通位，再置入
//! 新位；who 覆盖 u 时同时清 setuid，覆盖 g 时清 setgid，覆盖 o 时清 sticky。
//! 省略 who 的写法受 umask 约束：umask 中已置的位既不会被加上，也不会被 `=`
//! 清掉，因此同一个 `=rw` 在不同 umask 下得到不同结果。`X` 是条件执行位，只有
//! 目录或已有任一执行位时才展开成 x。`u=g` 一类拷贝按子句开始时的快照取值，
//! 同一子句内多次拷贝不会相互污染。
//!
//! 还有一条独立于解析的后置规则：普通文件在失去组执行位后必须同时失去 setgid，
//! 否则会留下一个“没有组执行却有组特权”的畸形模式；目录不受此规则约束，因为
//! 目录的 setgid 表示新建子项继承组，与执行位无必然关系。模型把这条规则放在
//! 所有子句应用之后统一执行一次，保证 `g-x,g+s` 与 `g+s,g-x` 的最终结果一致。
//!
//! 权限判定先于修改：非 root 调用者必须是文件属主，否则 EPERM；只读挂载或
//! 节点自带只读标志返回 EROFS；路径不存在返回 ENOENT。成功修改会写入日志，
//! 失败的调用只增加拒绝计数，不改变任何节点。十六格账本十成一败，覆盖八进制、
//! 符号置位、符号清除、拷贝、条件位、目录 setgid、普通文件 setgid 自动清除以及
//! 五类失败。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20af0;
/// 编号非法。
pub const CM_EID: u32 = SEG_BASE + 0x01;
/// 顺序错误。
pub const CM_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const CM_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const CM_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测缺失。
pub const CM_ENOBS: u32 = SEG_BASE + 0x05;
/// 结果不符。
pub const CM_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const CM_EBAL: u32 = SEG_BASE + 0x07;
/// 节点不存在。
pub const CM_ENOENT: u32 = SEG_BASE + 0x08;
/// 非属主。
pub const CM_EPERM: u32 = SEG_BASE + 0x09;
/// 语法非法。
pub const CM_EINVAL: u32 = SEG_BASE + 0x0a;
/// 只读挂载。
pub const CM_EROFS: u32 = SEG_BASE + 0x0b;
/// 票据不一致。
pub const CM_ESEAL: u32 = SEG_BASE + 0x0c;

/// setuid 位。
pub const S_ISUID: u16 = 0o4000;
/// setgid 位。
pub const S_ISGID: u16 = 0o2000;
/// sticky 位。
pub const S_STICKY: u16 = 0o1000;
/// 模式上限。
pub const MODE_MAX: u16 = 0o7777;
/// who：属主。
pub const WHO_U: u8 = 1;
/// who：属组。
pub const WHO_G: u8 = 2;
/// who：其他。
pub const WHO_O: u8 = 4;
/// who：全部。
pub const WHO_A: u8 = 7;
/// 普通文件。
pub const K_FILE: u8 = 1;
/// 目录。
pub const K_DIR: u8 = 2;

/// 样本格数。
pub const SAMPLES: usize = 16;
/// 名字容量。
pub const NAME_CAP: usize = 8;
/// 模式串容量。
pub const SPEC_CAP: usize = 12;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

fn fold(h: u64, v: u64) -> u64 {
    let mut h = h;
    let mut i = 0u32;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xff;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

fn cut(a: &[u8], n: u8) -> &[u8] {
    let k = n as usize;
    if k > a.len() {
        return &[];
    }
    match a.get(..k) {
        Some(v) => v,
        None => &[],
    }
}

fn put(dst: &mut [u8], src: &[u8]) -> u8 {
    let mut i = 0usize;
    while i < src.len() && i < dst.len() {
        dst[i] = src[i];
        i += 1;
    }
    i as u8
}

/// 一个索引节点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inode {
    pub name: [u8; NAME_CAP],
    pub nlen: u8,
    pub kind: u8,
    pub uid: u16,
    pub mode: u16,
    pub ro: bool,
}

impl Inode {
    pub fn new(name: &[u8], kind: u8, uid: u16, mode: u16, ro: bool) -> Inode {
        let mut n = [0u8; NAME_CAP];
        let nl = put(&mut n, name);
        Inode { name: n, nlen: nl, kind, uid, mode, ro }
    }

    pub fn name(&self) -> &[u8] {
        cut(&self.name, self.nlen)
    }
}

/// 一次成功修改。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    pub node: u8,
    pub before: u16,
    pub after: u16,
}

/// 文件系统视图。
pub struct Fs {
    nodes: Vec<Inode>,
    journal: Vec<Step>,
    ro_all: bool,
    applies: u32,
    rejects: u32,
}

impl Fs {
    pub fn new() -> Fs {
        let mut nodes = Vec::new();
        nodes.push(Inode::new(b"f", K_FILE, 0, 0o644, false));
        nodes.push(Inode::new(b"d", K_DIR, 0, 0o755, false));
        nodes.push(Inode::new(b"own", K_FILE, 7, 0o600, false));
        nodes.push(Inode::new(b"ro", K_FILE, 0, 0o644, true));
        nodes.push(Inode::new(b"sg", K_FILE, 0, 0o6755, false));
        nodes.push(Inode::new(b"gx", K_FILE, 0, 0o640, false));
        Fs { nodes, journal: Vec::new(), ro_all: false, applies: 0, rejects: 0 }
    }

    pub fn node(&self, i: usize) -> Option<Inode> {
        match self.nodes.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    pub fn count(&self) -> usize {
        self.nodes.len()
    }

    pub fn journal(&self) -> usize {
        self.journal.len()
    }

    pub fn applies(&self) -> u32 {
        self.applies
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    pub fn find(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                if n.name() == name {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    pub fn mode_of(&self, name: &[u8]) -> Result<u16, u32> {
        match self.find(name) {
            Some(i) => match self.nodes.get(i) {
                Some(n) => Ok(n.mode),
                None => Err(CM_ENOENT),
            },
            None => Err(CM_ENOENT),
        }
    }

    /// 执行一次 chmod。umask 只在 who 缺省时参与。
    pub fn chmod(&mut self, caller: u16, name: &[u8], spec: &[u8], umask: u16) -> Result<u16, u32> {
        let idx = match self.find(name) {
            Some(v) => v,
            None => {
                self.rejects += 1;
                return Err(CM_ENOENT);
            }
        };
        let old = match self.nodes.get(idx) {
            Some(n) => *n,
            None => {
                self.rejects += 1;
                return Err(CM_ENOENT);
            }
        };
        if self.ro_all || old.ro {
            self.rejects += 1;
            return Err(CM_EROFS);
        }
        if caller != 0 && caller != old.uid {
            self.rejects += 1;
            return Err(CM_EPERM);
        }
        let parsed = match parse_mode(spec) {
            Ok(v) => v,
            Err(e) => {
                self.rejects += 1;
                return Err(e);
            }
        };
        let new_mode = match parsed {
            Parsed::Octal(v) => v,
            Parsed::Symbolic => match symbolic(old.mode, spec, old.kind == K_DIR, umask) {
                Ok(v) => v,
                Err(e) => {
                    self.rejects += 1;
                    return Err(e);
                }
            },
        };
        if let Some(n) = self.nodes.get_mut(idx) {
            n.mode = new_mode;
        }
        self.journal.push(Step { node: idx as u8, before: old.mode, after: new_mode });
        self.applies += 1;
        Ok(new_mode)
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.nodes.is_empty() || self.journal.len() != self.applies as usize {
            return Err(CM_EBAL);
        }
        let mut i = 0usize;
        while i < self.nodes.len() {
            let n = match self.nodes.get(i) {
                Some(v) => *v,
                None => return Err(CM_ENOBS),
            };
            if n.nlen == 0 || n.mode > MODE_MAX || (n.kind != K_FILE && n.kind != K_DIR) {
                return Err(CM_EBAL);
            }
            if n.kind == K_FILE && (n.mode & S_ISGID) != 0 && (n.mode & 0o010) == 0 {
                return Err(CM_EBAL);
            }
            let mut j = i + 1;
            while j < self.nodes.len() {
                if let Some(m) = self.nodes.get(j) {
                    if n.name() == m.name() {
                        return Err(CM_EBAL);
                    }
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                h = fold(h, n.mode as u64);
                h = fold(h, n.uid as u64);
                h = fold(h, n.kind as u64);
                h = fold(h, n.ro as u64);
                let mut k = 0usize;
                while k < n.nlen as usize {
                    if let Some(b) = n.name.get(k) {
                        h ^= *b as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        h = fold(h, self.journal.len() as u64);
        h = fold(h, self.applies as u64);
        fold(h, self.rejects as u64)
    }
}

/// 解析结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parsed {
    Octal(u16),
    Symbolic,
}

/// 判断是否为八进制写法并取值。
pub fn parse_mode(spec: &[u8]) -> Result<Parsed, u32> {
    if spec.is_empty() {
        return Err(CM_EINVAL);
    }
    let first = match spec.first() {
        Some(v) => *v,
        None => return Err(CM_EINVAL),
    };
    if first.is_ascii_digit() {
        if spec.len() > 5 {
            return Err(CM_EINVAL);
        }
        let mut v = 0u32;
        let mut i = 0usize;
        while i < spec.len() {
            let b = match spec.get(i) {
                Some(x) => *x,
                None => return Err(CM_EINVAL),
            };
            if !b.is_ascii_digit() {
                return Err(CM_EINVAL);
            }
            let d = (b - b'0') as u32;
            if d > 7 {
                return Err(CM_EINVAL);
            }
            v = v * 8 + d;
            if v > MODE_MAX as u32 {
                return Err(CM_EINVAL);
            }
            i += 1;
        }
        Ok(Parsed::Octal(v as u16))
    } else {
        Ok(Parsed::Symbolic)
    }
}

fn spread(who: u8) -> u16 {
    let mut v = 0u16;
    if who & WHO_U != 0 {
        v |= 0o700;
    }
    if who & WHO_G != 0 {
        v |= 0o070;
    }
    if who & WHO_O != 0 {
        v |= 0o007;
    }
    v
}

fn specials(who: u8) -> u16 {
    let mut v = 0u16;
    if who & WHO_U != 0 {
        v |= S_ISUID;
    }
    if who & WHO_G != 0 {
        v |= S_ISGID;
    }
    if who & WHO_O != 0 {
        v |= S_STICKY;
    }
    v
}

/// 按符号子句应用模式串。
pub fn symbolic(mut m: u16, spec: &[u8], is_dir: bool, umask: u16) -> Result<u16, u32> {
    if spec.is_empty() {
        return Err(CM_EINVAL);
    }
    let um = umask & 0o777;
    let mut i = 0usize;
    while i < spec.len() {
        let mut who = 0u8;
        while i < spec.len() {
            let b = match spec.get(i) {
                Some(v) => *v,
                None => return Err(CM_EINVAL),
            };
            if b == b'u' {
                who |= WHO_U;
            } else if b == b'g' {
                who |= WHO_G;
            } else if b == b'o' {
                who |= WHO_O;
            } else if b == b'a' {
                who = WHO_A;
            } else {
                break;
            }
            i += 1;
        }
        let omitted = who == 0;
        if omitted {
            who = WHO_A;
        }
        let op = match spec.get(i) {
            Some(v) => *v,
            None => return Err(CM_EINVAL),
        };
        if op != b'+' && op != b'-' && op != b'=' {
            return Err(CM_EINVAL);
        }
        i += 1;
        let mut add = 0u16;
        let mut copy = 0u8;
        let mut cond_x = false;
        let mut any = false;
        while i < spec.len() {
            let b = match spec.get(i) {
                Some(v) => *v,
                None => return Err(CM_EINVAL),
            };
            if b == b',' {
                break;
            }
            any = true;
            if b == b'r' {
                if who & WHO_U != 0 {
                    add |= 0o400;
                }
                if who & WHO_G != 0 {
                    add |= 0o040;
                }
                if who & WHO_O != 0 {
                    add |= 0o004;
                }
            } else if b == b'w' {
                if who & WHO_U != 0 {
                    add |= 0o200;
                }
                if who & WHO_G != 0 {
                    add |= 0o020;
                }
                if who & WHO_O != 0 {
                    add |= 0o002;
                }
            } else if b == b'x' {
                if who & WHO_U != 0 {
                    add |= 0o100;
                }
                if who & WHO_G != 0 {
                    add |= 0o010;
                }
                if who & WHO_O != 0 {
                    add |= 0o001;
                }
            } else if b == b'X' {
                cond_x = true;
            } else if b == b's' {
                if who & WHO_U != 0 {
                    add |= S_ISUID;
                }
                if who & WHO_G != 0 {
                    add |= S_ISGID;
                }
            } else if b == b't' {
                if who & WHO_O != 0 {
                    add |= S_STICKY;
                }
            } else if b == b'u' {
                copy = 1;
            } else if b == b'g' {
                copy = 2;
            } else if b == b'o' {
                copy = 3;
            } else {
                return Err(CM_EINVAL);
            }
            i += 1;
        }
        if !any {
            return Err(CM_EINVAL);
        }
        if copy != 0 {
            let shift = if copy == 1 {
                6
            } else if copy == 2 {
                3
            } else {
                0
            };
            let src = (m >> shift) & 0o7;
            if who & WHO_U != 0 {
                add |= src << 6;
            }
            if who & WHO_G != 0 {
                add |= src << 3;
            }
            if who & WHO_O != 0 {
                add |= src;
            }
        }
        if cond_x && (is_dir || (m & 0o111) != 0) {
            if who & WHO_U != 0 {
                add |= 0o100;
            }
            if who & WHO_G != 0 {
                add |= 0o010;
            }
            if who & WHO_O != 0 {
                add |= 0o001;
            }
        }
        if omitted {
            add &= !um;
        }
        if op == b'+' {
            m |= add;
        } else if op == b'-' {
            m &= !add;
        } else {
            let mut clear = spread(who) | specials(who);
            if omitted {
                clear &= !um;
            }
            m &= !clear;
            m |= add;
        }
        if i < spec.len() {
            if spec.get(i) != Some(&b',') {
                return Err(CM_EINVAL);
            }
            i += 1;
            if i >= spec.len() {
                return Err(CM_EINVAL);
            }
        }
    }
    if !is_dir && (m & 0o010) == 0 {
        m &= !S_ISGID;
    }
    Ok(m & MODE_MAX)
}

/// 一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    pub name: [u8; NAME_CAP],
    pub nlen: u8,
    pub spec: [u8; SPEC_CAP],
    pub slen: u8,
    pub caller: u16,
    pub umask: u16,
    pub expect: u16,
    pub ok: bool,
}

impl Case {
    pub fn name(&self) -> &[u8] {
        cut(&self.name, self.nlen)
    }

    pub fn spec(&self) -> &[u8] {
        cut(&self.spec, self.slen)
    }
}

/// 结局。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ending {
    pub mode: u16,
    pub code: u16,
}

/// 账本。
pub struct Ledger {
    fs: Fs,
    cases: Vec<Case>,
    ends: Vec<Ending>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger { fs: Fs::new(), cases: Vec::new(), ends: Vec::new(), frozen: false, root: 0, rejects: 0 }
    }

    pub fn register(&mut self, slot: u8, caller: u16, name: &[u8], spec: &[u8], umask: u16, expect: u16, ok: bool) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(CM_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(CM_EID);
        }
        if (slot as usize) < self.cases.len() {
            self.rejects += 1;
            return Err(CM_EDUP);
        }
        if (slot as usize) != self.cases.len() {
            self.rejects += 1;
            return Err(CM_EORDER);
        }
        let r = self.fs.chmod(caller, name, spec, umask);
        let end = match r {
            Ok(m) => {
                if !ok || m != expect {
                    self.rejects += 1;
                    return Err(CM_EMISMATCH);
                }
                Ending { mode: m, code: 0 }
            }
            Err(e) => {
                if ok {
                    self.rejects += 1;
                    return Err(CM_EMISMATCH);
                }
                Ending { mode: 0, code: (e & 0xffff) as u16 }
            }
        };
        let mut c = Case { name: [0; NAME_CAP], nlen: 0, spec: [0; SPEC_CAP], slen: 0, caller, umask, expect, ok };
        c.nlen = put(&mut c.name, name);
        c.slen = put(&mut c.spec, spec);
        self.cases.push(c);
        self.ends.push(end);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.cases.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }

    pub fn case(&self, i: usize) -> Option<Case> {
        match self.cases.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    pub fn ending(&self, i: usize) -> Option<Ending> {
        match self.ends.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    pub fn fs(&self) -> &Fs {
        &self.fs
    }

    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.ends.len() {
            if let Some(e) = self.ends.get(i) {
                if e.code == 0 {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    pub fn err_count(&self) -> usize {
        self.ends.len() - self.ok_count()
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.cases.len() != self.ends.len() || self.cases.is_empty() {
            return Err(CM_EBAL);
        }
        let mut i = 0usize;
        while i < self.cases.len() {
            let c = match self.cases.get(i) {
                Some(v) => *v,
                None => return Err(CM_ENOBS),
            };
            let e = match self.ends.get(i) {
                Some(v) => *v,
                None => return Err(CM_ENOBS),
            };
            if c.ok != (e.code == 0) || (c.ok && e.mode != c.expect) {
                return Err(CM_EMISMATCH);
            }
            i += 1;
        }
        self.fs.audit()
    }

    pub fn seal(&self) -> u64 {
        self.fs.seal()
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(CM_EFROZEN);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
        } else {
            Err(CM_ESEAL)
        }
    }

    pub fn root(&self) -> u64 {
        self.root
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 构造十六格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let per = (CM_EPERM & 0xffff) as u16;
    let rov = (CM_EROFS & 0xffff) as u16;
    let inv = (CM_EINVAL & 0xffff) as u16;
    let noe = (CM_ENOENT & 0xffff) as u16;
    let _ = l.register(0, 0, b"f", b"u+x", 0o022, 0o744, true);
    let _ = l.register(1, 0, b"f", b"go-r", 0o022, 0o700, true);
    let _ = l.register(2, 0, b"f", b"a=r", 0o022, 0o444, true);
    let _ = l.register(3, 0, b"f", b"ug+w", 0o022, 0o664, true);
    let _ = l.register(4, 0, b"f", b"a+X", 0o022, 0o664, true);
    let _ = l.register(5, 0, b"d", b"a+X", 0o022, 0o755, true);
    let _ = l.register(6, 0, b"d", b"g+s", 0o022, 0o2755, true);
    let _ = l.register(7, 0, b"d", b"g-x", 0o022, 0o2745, true);
    let _ = l.register(8, 0, b"sg", b"g-x", 0o022, 0o4745, true);
    let _ = l.register(9, 7, b"own", b"0640", 0o022, 0o640, true);
    let _ = l.register(10, 9, b"f", b"u+x", 0o022, per, false);
    let _ = l.register(11, 0, b"ro", b"u+x", 0o022, rov, false);
    let _ = l.register(12, 0, b"f", b"u+z", 0o022, inv, false);
    let _ = l.register(13, 0, b"f", b"u+rwx,", 0o022, inv, false);
    let _ = l.register(14, 0, b"nope", b"u+x", 0o022, noe, false);
    let _ = l.register(15, 0, b"f", b"65536", 0o022, inv, false);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.ok_count() == 10 && l.err_count() == 6 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    out[0] = base && root != 0 && rep == Ok(root);

    out[1] = parse_mode(b"0640") == Ok(Parsed::Octal(0o640))
        && parse_mode(b"7777") == Ok(Parsed::Octal(0o7777))
        && parse_mode(b"8") == Err(CM_EINVAL)
        && parse_mode(b"10000") == Err(CM_EINVAL)
        && parse_mode(b"") == Err(CM_EINVAL);

    out[2] = symbolic(0o640, b"u=g", false, 0o022) == Ok(0o440)
        && symbolic(0o755, b"a+X", false, 0o022) == Ok(0o755)
        && symbolic(0o000, b"a+X", false, 0o022) == Ok(0o000)
        && symbolic(0o000, b"a+X", true, 0o022) == Ok(0o111);

    let mut f = Fs::new();
    let m1 = f.chmod(0, b"sg", b"g-x", 0);
    let m2 = f.chmod(0, b"d", b"g+s", 0);
    let m3 = f.chmod(0, b"f", b"=r", 0o077);
    out[3] = m1 == Ok(0o4745) && m2 == Ok(0o2755) && m3 == Ok(0o444) && f.audit().is_ok();
    out
}

/// 十三判据。
pub fn run_unx11435_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11435.chmod");
    let l = build_ledger();
    s.add("CM1-十六格齐备", l.len() == SAMPLES && !l.is_empty(), "static");
    s.add_dyn("CM2-十成六拒", l.ok_count() == 10 && l.err_count() == 6 && l.audit().is_ok(), &format!("ok={} err={}", l.ok_count(), l.err_count()));
    s.add("CM3-符号置位链", l.ending(0).map(|e| e.mode) == Some(0o744) && l.ending(1).map(|e| e.mode) == Some(0o700), "static");
    s.add("CM4-等号清空", l.ending(2).map(|e| e.mode) == Some(0o444) && l.ending(3).map(|e| e.mode) == Some(0o664), "static");
    s.add("CM5-条件执行位", l.ending(4).map(|e| e.mode) == Some(0o664) && l.ending(5).map(|e| e.mode) == Some(0o755), "static");
    s.add("CM6-目录保留setgid", l.ending(6).map(|e| e.mode) == Some(0o2755) && l.ending(7).map(|e| e.mode) == Some(0o2745), "static");
    s.add("CM7-普通文件清setgid", l.ending(8).map(|e| e.mode) == Some(0o4745), "static");
    s.add("CM8-八进制路径", l.ending(9).map(|e| e.mode) == Some(0o640) && l.ending(9).map(|e| e.code) == Some(0), "static");
    s.add("CM9-非属主拒绝", l.ending(10).map(|e| e.code) == Some((CM_EPERM & 0xffff) as u16), "static");
    s.add("CM10-只读拒绝", l.ending(11).map(|e| e.code) == Some((CM_EROFS & 0xffff) as u16), "static");
    s.add("CM11-语法与越界", l.ending(12).map(|e| e.code) == Some((CM_EINVAL & 0xffff) as u16) && l.ending(13).map(|e| e.code) == Some((CM_EINVAL & 0xffff) as u16) && l.ending(15).map(|e| e.code) == Some((CM_EINVAL & 0xffff) as u16), "static");
    s.add("CM12-缺失节点", l.ending(14).map(|e| e.code) == Some((CM_ENOENT & 0xffff) as u16) && l.fs().mode_of(b"f") == Ok(0o664), "static");
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(CM_EFROZEN);
    let inj = inject_four();
    s.add_dyn("CM13-冻结重放四注入", root != 0 && rep == Ok(root) && again && inj == [true; 4], &format!("root={:016x} {:?}", root, inj));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn octal_parse() {
        assert_eq!(parse_mode(b"640"), Ok(Parsed::Octal(0o640)));
        assert_eq!(parse_mode(b"07777"), Ok(Parsed::Octal(0o7777)));
        assert_eq!(parse_mode(b"9"), Err(CM_EINVAL));
        assert_eq!(parse_mode(b"77777"), Err(CM_EINVAL));
    }

    #[test]
    fn symbolic_add_remove() {
        assert_eq!(symbolic(0o644, b"u+x,g-w", false, 0), Ok(0o744));
        assert_eq!(symbolic(0o777, b"go-rwx", false, 0), Ok(0o700));
    }

    #[test]
    fn copy_classes() {
        assert_eq!(symbolic(0o640, b"u=g", false, 0), Ok(0o440));
        assert_eq!(symbolic(0o640, b"o=u", false, 0), Ok(0o646));
    }

    #[test]
    fn conditional_x() {
        assert_eq!(symbolic(0o600, b"u+X", false, 0), Ok(0o600));
        assert_eq!(symbolic(0o700, b"go+X", false, 0), Ok(0o711));
        assert_eq!(symbolic(0o000, b"a+X", true, 0), Ok(0o111));
    }

    #[test]
    fn setgid_rules() {
        assert_eq!(symbolic(0o6755, b"g-x", false, 0), Ok(0o4745));
        assert_eq!(symbolic(0o2755, b"g-x", true, 0), Ok(0o2745));
    }

    #[test]
    fn umask_only_when_omitted() {
        assert_eq!(symbolic(0o000, b"+w", false, 0o022), Ok(0o200));
        assert_eq!(symbolic(0o000, b"u+w", false, 0o022), Ok(0o200));
        assert_eq!(symbolic(0o777, b"=r", false, 0o022), Ok(0o466));
        assert_eq!(symbolic(0o777, b"a=r", false, 0o022), Ok(0o444));
    }

    #[test]
    fn errors_and_ledger() {
        assert_eq!(symbolic(0o644, b"u+z", false, 0), Err(CM_EINVAL));
        assert_eq!(symbolic(0o644, b"u+rw,", false, 0), Err(CM_EINVAL));
        assert_eq!(symbolic(0o644, b",u+r", false, 0), Err(CM_EINVAL));
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert!(l.audit().is_ok());
        assert_eq!(l.fs().mode_of(b"f"), Ok(0o664));
        assert_eq!(inject_four(), [true; 4]);
        assert_eq!(run_unx11435_checks().failed_count(), 0);
    }
}
