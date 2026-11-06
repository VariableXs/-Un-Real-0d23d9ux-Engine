//! UNX-F11436 umask 创建掩码（0x20b00 段）
//!
//! umask 不是权限位本身，而是进程在创建节点时对请求模式做的一次按位清除。模型
//! 把进程状态、掩码历史与节点出生记录放在一起：每次设置掩码都要保留旧值，因为
//! umask() 系统调用返回的是设置之前的值；每个节点记录请求模式、出生掩码与出生
//! 模式，出生模式恒等于请求模式与出生掩码取反后的与。
//!
//! 掩码有两种写法。八进制写法最多四位且不超过 0777；符号写法形如
//! `u=rwx,g=rx,o=`，其中 who 缺省视为 a，操作符只允许 `=`，权限字母只允许 rwx，
//! 未列出的类保持全清。两种写法最终都归约成一个 9 位清除掩码，非法字符、越界
//! 数值和空子句返回 EINVAL，并且不改变当前掩码。
//!
//! 时序是本模型的重点。创建之后的 chmod 完全绕过掩码，因此同一个文件可以先是
//! 0644、再被改成 0666；反过来先 chmod 再改掩码不会追溯已有节点。符号掩码
//! `u=rwx,g=,o=` 等价于 0770，用它创建 0666 会得到 0006，这个结果只由出生时刻
//! 的掩码决定，与之后的任何掩码设置无关。模型用三段掩码纪元（022、077、0770、
//! 002、0）交叉验证这条规则。
//!
//! 失败路径独立计数：重复创建同名节点返回 EEXIST，chmod 不存在的名字返回
//! ENOENT，请求模式超过 07777 返回 EINVAL，非法掩码返回 EINVAL 且保留旧掩码。
//! 审计要求节点名唯一、出生模式可复算、当前模式等于出生模式（未 chmod 时），
//! 账本十六格十二成一败。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20b00;
/// 编号非法。
pub const UM_EID: u32 = SEG_BASE + 0x01;
/// 顺序错误。
pub const UM_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const UM_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const UM_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测缺失。
pub const UM_ENOBS: u32 = SEG_BASE + 0x05;
/// 结果不符。
pub const UM_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const UM_EBAL: u32 = SEG_BASE + 0x07;
/// 名字已存在。
pub const UM_EEXIST: u32 = SEG_BASE + 0x08;
/// 节点不存在。
pub const UM_ENOENT: u32 = SEG_BASE + 0x09;
/// 参数非法。
pub const UM_EINVAL: u32 = SEG_BASE + 0x0a;
/// 票据不一致。
pub const UM_ESEAL: u32 = SEG_BASE + 0x0b;

/// 操作：设置掩码。
pub const OP_MASK: u8 = 1;
/// 操作：创建节点。
pub const OP_CREATE: u8 = 2;
/// 操作：修改模式。
pub const OP_CHMOD: u8 = 3;
/// 普通文件。
pub const K_FILE: u8 = 1;
/// 目录。
pub const K_DIR: u8 = 2;

/// 掩码上限。
pub const MASK_MAX: u16 = 0o777;
/// 模式上限。
pub const MODE_MAX: u16 = 0o7777;
/// 格数。
pub const SAMPLES: usize = 16;
/// 名字容量。
pub const NAME_CAP: usize = 8;
/// 账本容量。
pub const FILE_CAP: usize = 8;
/// 掩码历史容量。
pub const EPOCH_CAP: usize = 8;

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

/// 一个节点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct File {
    pub name: [u8; NAME_CAP],
    pub nlen: u8,
    pub kind: u8,
    pub req: u16,
    pub brmask: u16,
    pub born: u16,
    pub mode: u16,
    pub touched: bool,
}

impl File {
    pub fn name(&self) -> &[u8] {
        cut(&self.name, self.nlen)
    }
}

/// 进程视图。
pub struct Proc {
    mask: u16,
    history: Vec<u16>,
    files: Vec<File>,
    creates: u32,
    rejects: u32,
}

impl Proc {
    pub fn new() -> Proc {
        Proc { mask: 0, history: Vec::new(), files: Vec::new(), creates: 0, rejects: 0 }
    }

    pub fn mask(&self) -> u16 {
        self.mask
    }

    pub fn history(&self) -> &[u16] {
        &self.history
    }

    pub fn files(&self) -> &[File] {
        &self.files
    }

    pub fn creates(&self) -> u32 {
        self.creates
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    pub fn find(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.files.len() {
            if let Some(f) = self.files.get(i) {
                if f.name() == name {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 设置掩码，返回旧值。
    pub fn set_mask(&mut self, m: u16) -> Result<u16, u32> {
        if m > MASK_MAX || self.history.len() >= EPOCH_CAP {
            self.rejects += 1;
            return Err(UM_EINVAL);
        }
        let old = self.mask;
        self.history.push(old);
        self.mask = m;
        Ok(old)
    }

    /// 按 ASCII 掩码描述设置，返回旧值。
    pub fn set_mask_spec(&mut self, spec: &[u8]) -> Result<u16, u32> {
        let m = match parse_mask(spec) {
            Ok(v) => v,
            Err(e) => {
                self.rejects += 1;
                return Err(e);
            }
        };
        self.set_mask(m)
    }

    /// 创建节点，模式为请求值与掩码取反后的与。
    pub fn create(&mut self, name: &[u8], kind: u8, req: u16) -> Result<u16, u32> {
        if name.is_empty() || req > MODE_MAX || self.files.len() >= FILE_CAP {
            self.rejects += 1;
            return Err(UM_EINVAL);
        }
        if self.find(name).is_some() {
            self.rejects += 1;
            return Err(UM_EEXIST);
        }
        let born = req & !self.mask;
        let mut f = File { name: [0; NAME_CAP], nlen: 0, kind, req, brmask: self.mask, born, mode: born, touched: false };
        f.nlen = put(&mut f.name, name);
        self.files.push(f);
        self.creates += 1;
        Ok(born)
    }

    /// 修改模式，绕过掩码。
    pub fn chmod(&mut self, name: &[u8], mode: u16) -> Result<u16, u32> {
        if mode > MODE_MAX {
            self.rejects += 1;
            return Err(UM_EINVAL);
        }
        let i = match self.find(name) {
            Some(v) => v,
            None => {
                self.rejects += 1;
                return Err(UM_ENOENT);
            }
        };
        if let Some(f) = self.files.get_mut(i) {
            f.mode = mode;
            f.touched = true;
        }
        Ok(mode)
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.files.is_empty() || self.files.len() > FILE_CAP || self.history.len() > 5 {
            return Err(UM_EBAL);
        }
        let mut i = 0usize;
        while i < self.files.len() {
            let f = match self.files.get(i) {
                Some(v) => *v,
                None => return Err(UM_ENOBS),
            };
            if f.born != (f.req & !f.brmask) || f.born > MODE_MAX || f.mode > MODE_MAX {
                return Err(UM_EBAL);
            }
            if !f.touched && f.mode != f.born {
                return Err(UM_EBAL);
            }
            if f.kind != K_FILE && f.kind != K_DIR {
                return Err(UM_EBAL);
            }
            let mut j = i + 1;
            while j < self.files.len() {
                if let Some(g) = self.files.get(j) {
                    if f.name() == g.name() {
                        return Err(UM_EBAL);
                    }
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    pub fn seal(&self) -> u64 {
        let mut h = fold(FNV_OFF, self.mask as u64);
        let mut i = 0usize;
        while i < self.history.len() {
            if let Some(m) = self.history.get(i) {
                h = fold(h, *m as u64);
            }
            i += 1;
        }
        i = 0;
        while i < self.files.len() {
            if let Some(f) = self.files.get(i) {
                h = fold(h, f.req as u64);
                h = fold(h, f.brmask as u64);
                h = fold(h, f.born as u64);
                h = fold(h, f.mode as u64);
                h = fold(h, f.kind as u64);
                h = fold(h, f.touched as u64);
                let mut k = 0usize;
                while k < f.nlen as usize {
                    if let Some(b) = f.name.get(k) {
                        h ^= *b as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        h = fold(h, self.creates as u64);
        fold(h, self.rejects as u64)
    }
}

/// 解析八进制或符号掩码。
pub fn parse_mask(spec: &[u8]) -> Result<u16, u32> {
    let first = match spec.first() {
        Some(v) => *v,
        None => return Err(UM_EINVAL),
    };
    if first.is_ascii_digit() {
        if spec.len() > 4 {
            return Err(UM_EINVAL);
        }
        let mut v = 0u16;
        let mut i = 0usize;
        while i < spec.len() {
            let b = match spec.get(i) {
                Some(x) => *x,
                None => return Err(UM_EINVAL),
            };
            if !b.is_ascii_digit() {
                return Err(UM_EINVAL);
            }
            let d = (b - b'0') as u16;
            if d > 7 {
                return Err(UM_EINVAL);
            }
            v = v * 8 + d;
            i += 1;
        }
        if v > MASK_MAX {
            return Err(UM_EINVAL);
        }
        Ok(v)
    } else {
        let mut allowed = 0u16;
        let mut i = 0usize;
        while i < spec.len() {
            let mut who = 0u8;
            while i < spec.len() {
                let b = match spec.get(i) {
                    Some(x) => *x,
                    None => return Err(UM_EINVAL),
                };
                if b == b'u' {
                    who |= 1;
                } else if b == b'g' {
                    who |= 2;
                } else if b == b'o' {
                    who |= 4;
                } else if b == b'a' {
                    who = 7;
                } else {
                    break;
                }
                i += 1;
            }
            if who == 0 {
                who = 7;
            }
            if spec.get(i) != Some(&b'=') {
                return Err(UM_EINVAL);
            }
            i += 1;
            let mut perm = 0u16;
            while i < spec.len() {
                let b = match spec.get(i) {
                    Some(x) => *x,
                    None => return Err(UM_EINVAL),
                };
                if b == b',' {
                    break;
                }
                if b == b'r' {
                    perm |= 0o444;
                } else if b == b'w' {
                    perm |= 0o222;
                } else if b == b'x' {
                    perm |= 0o111;
                } else {
                    return Err(UM_EINVAL);
                }
                i += 1;
            }
            if who & 1 != 0 {
                allowed |= perm & 0o700;
            }
            if who & 2 != 0 {
                allowed |= perm & 0o070;
            }
            if who & 4 != 0 {
                allowed |= perm & 0o007;
            }
            if i < spec.len() {
                if spec.get(i) != Some(&b',') {
                    return Err(UM_EINVAL);
                }
                i += 1;
                if i >= spec.len() {
                    return Err(UM_EINVAL);
                }
            }
        }
        Ok(MASK_MAX & !allowed)
    }
}

/// 一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    pub op: u8,
    pub name: [u8; NAME_CAP],
    pub nlen: u8,
    pub arg: u16,
    pub spec: [u8; 12],
    pub slen: u8,
    pub kind: u8,
    pub expect: u16,
    pub ok: bool,
}

/// 结局。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ending {
    pub value: u16,
    pub code: u16,
}

/// 账本。
pub struct Ledger {
    p: Proc,
    cases: Vec<Case>,
    ends: Vec<Ending>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger { p: Proc::new(), cases: Vec::new(), ends: Vec::new(), frozen: false, root: 0, rejects: 0 }
    }

    pub fn register_mask(&mut self, slot: u8, spec: &[u8], expect: u16, ok: bool) -> Result<(), u32> {
        self.begin(slot)?;
        let r = self.p.set_mask_spec(spec);
        self.finish(r, spec, expect, ok, 0)
    }

    pub fn register_create(&mut self, slot: u8, name: &[u8], kind: u8, req: u16, expect: u16, ok: bool) -> Result<(), u32> {
        self.begin(slot)?;
        let r = self.p.create(name, kind, req);
        self.finish(r, name, expect, ok, kind)
    }

    pub fn register_chmod(&mut self, slot: u8, name: &[u8], mode: u16, expect: u16, ok: bool) -> Result<(), u32> {
        self.begin(slot)?;
        let r = self.p.chmod(name, mode);
        self.finish(r, name, expect, ok, 0)
    }

    fn begin(&mut self, slot: u8) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(UM_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(UM_EID);
        }
        if (slot as usize) < self.cases.len() {
            self.rejects += 1;
            return Err(UM_EDUP);
        }
        if (slot as usize) != self.cases.len() {
            self.rejects += 1;
            return Err(UM_EORDER);
        }
        Ok(())
    }

    fn finish(&mut self, r: Result<u16, u32>, arg: &[u8], expect: u16, ok: bool, kind: u8) -> Result<(), u32> {
        let end = match r {
            Ok(v) => {
                if !ok || v != expect {
                    self.rejects += 1;
                    return Err(UM_EMISMATCH);
                }
                Ending { value: v, code: 0 }
            }
            Err(e) => {
                let c = (e & 0xffff) as u16;
                if ok || c != expect {
                    self.rejects += 1;
                    return Err(UM_EMISMATCH);
                }
                Ending { value: 0, code: c }
            }
        };
        let mut c = Case { op: 0, name: [0; NAME_CAP], nlen: 0, arg: 0, spec: [0; 12], slen: 0, kind, expect, ok };
        c.nlen = put(&mut c.name, arg);
        self.cases.push(c);
        self.ends.push(end);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.cases.len()
    }

    pub fn ending(&self, i: usize) -> Option<Ending> {
        match self.ends.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    pub fn proc(&self) -> &Proc {
        &self.p
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
            return Err(UM_EBAL);
        }
        self.p.audit()
    }

    pub fn seal(&self) -> u64 {
        self.p.seal()
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(UM_EFROZEN);
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
            Err(UM_ESEAL)
        }
    }

    pub fn root(&self) -> u64 {
        self.root
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 十六格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let eex = (UM_EEXIST & 0xffff) as u16;
    let eno = (UM_ENOENT & 0xffff) as u16;
    let ein = (UM_EINVAL & 0xffff) as u16;
    let _ = l.register_mask(0, b"022", 0, true);
    let _ = l.register_create(1, b"f", K_FILE, 0o666, 0o644, true);
    let _ = l.register_create(2, b"d", K_DIR, 0o777, 0o755, true);
    let _ = l.register_mask(3, b"077", 0o022, true);
    let _ = l.register_create(4, b"g", K_FILE, 0o666, 0o600, true);
    let _ = l.register_chmod(5, b"f", 0o666, 0o666, true);
    let _ = l.register_create(6, b"h", K_FILE, 0o777, 0o700, true);
    let _ = l.register_mask(7, b"u=,g=,o=rwx", 0o077, true);
    let _ = l.register_create(8, b"k", K_FILE, 0o666, 0o006, true);
    let _ = l.register_mask(9, b"002", 0o770, true);
    let _ = l.register_create(10, b"m", K_FILE, 0o666, 0o664, true);
    let _ = l.register_mask(11, b"0", 0o002, true);
    let _ = l.register_create(12, b"z", K_FILE, 0o10000, ein, false);
    let _ = l.register_create(13, b"f", K_FILE, 0o600, eex, false);
    let _ = l.register_chmod(14, b"nope", 0o600, eno, false);
    let _ = l.register_mask(15, b"1000", ein, false);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.ok_count() == 12 && l.err_count() == 4 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    out[0] = base && root != 0 && rep == Ok(root);

    let mut a = Proc::new();
    let m1 = a.set_mask(0o022);
    let c1 = a.create(b"t", K_FILE, 0o666);
    let c2 = a.create(b"u", K_FILE, 0o777);
    out[1] = m1 == Ok(0) && c1 == Ok(0o644) && c2 == Ok(0o755) && a.audit().is_ok();

    let mut b = Proc::new();
    let chg = b.chmod(b"none", 0o600);
    let _ = b.set_mask(0o077);
    let c3 = b.create(b"v", K_FILE, 0o666);
    let c4 = b.chmod(b"v", 0o666);
    out[2] = chg == Err(UM_ENOENT) && c3 == Ok(0o600) && c4 == Ok(0o666) && b.mask() == 0o077;

    out[3] = parse_mask(b"077") == Ok(0o077)
        && parse_mask(b"u=rwx,g=rx,o=") == Ok(0o027)
        && parse_mask(b"u=rwx,g=,o=") == Ok(0o077)
        && parse_mask(b"7777") == Err(UM_EINVAL)
        && parse_mask(b"u+q") == Err(UM_EINVAL);
    out
}

/// 十三判据。
pub fn run_unx11436_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11436.umask");
    let l = build_ledger();
    s.add("UM1-十六格齐备", l.len() == SAMPLES, "static");
    s.add_dyn("UM2-十二成四拒", l.ok_count() == 12 && l.err_count() == 4 && l.audit().is_ok(), &format!("ok={} err={}", l.ok_count(), l.err_count()));
    s.add("UM3-首建文件", l.ending(1).map(|e| e.value) == Some(0o644) && l.proc().files().len() == 6, "static");
    s.add("UM4-首建目录", l.ending(2).map(|e| e.value) == Some(0o755), "static");
    s.add("UM5-收紧掩码", l.ending(3).map(|e| e.value) == Some(0o022) && l.ending(4).map(|e| e.value) == Some(0o600), "static");
    s.add("UM6-chmod绕过掩码", l.ending(5).map(|e| e.value) == Some(0o666), "static");
    s.add("UM7-第三纪元", l.ending(6).map(|e| e.value) == Some(0o700), "static");
    s.add("UM8-符号掩码", l.ending(7).map(|e| e.value) == Some(0o077) && l.ending(8).map(|e| e.value) == Some(0o006), "static");
    s.add("UM9-掩码历史", l.proc().history() == &[0u16, 0o022, 0o077, 0o770, 0o002][..] && l.proc().mask() == 0, "static");
    s.add("UM10-出生掩码复算", l.proc().files().get(4).map(|f| (f.req, f.brmask, f.born)) == Some((0o666, 0o770, 0o006)), "static");
    s.add("UM11-无效请求", l.ending(12).map(|e| e.code) == Some((UM_EINVAL & 0xffff) as u16), "static");
    s.add("UM12-冲突与缺失", l.ending(13).map(|e| e.code) == Some((UM_EEXIST & 0xffff) as u16) && l.ending(14).map(|e| e.code) == Some((UM_ENOENT & 0xffff) as u16), "static");
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(UM_EFROZEN);
    let inj = inject_four();
    s.add_dyn("UM13-冻结重放四注入", root != 0 && rep == Ok(root) && again && inj == [true; 4], &format!("root={:016x} {:?}", root, inj));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn octal_masks() {
        assert_eq!(parse_mask(b"022"), Ok(0o022));
        assert_eq!(parse_mask(b"0777"), Ok(0o777));
        assert_eq!(parse_mask(b"1000"), Err(UM_EINVAL));
        assert_eq!(parse_mask(b"8"), Err(UM_EINVAL));
    }

    #[test]
    fn symbolic_masks() {
        assert_eq!(parse_mask(b"u=rwx,g=rx,o="), Ok(0o027));
        assert_eq!(parse_mask(b"u=,g=,o=rwx"), Ok(0o770));
        assert_eq!(parse_mask(b"a=rwx"), Ok(0));
        assert_eq!(parse_mask(b"u=rw"), Ok(0o177));
    }

    #[test]
    fn create_applies_mask() {
        let mut p = Proc::new();
        assert!(p.set_mask(0o027).is_ok());
        assert_eq!(p.create(b"x", K_FILE, 0o666), Ok(0o640));
        assert_eq!(p.create(b"y", K_DIR, 0o777), Ok(0o750));
    }

    #[test]
    fn chmod_ignores_mask() {
        let mut p = Proc::new();
        assert!(p.set_mask(0o077).is_ok());
        assert_eq!(p.create(b"x", K_FILE, 0o666), Ok(0o600));
        assert_eq!(p.chmod(b"x", 0o666), Ok(0o666));
        assert!(p.set_mask(0o022).is_ok());
        assert_eq!(p.chmod(b"x", 0o644), Ok(0o644));
    }

    #[test]
    fn mask_returns_previous() {
        let mut p = Proc::new();
        assert_eq!(p.set_mask(0o022), Ok(0));
        assert_eq!(p.set_mask(0o077), Ok(0o022));
        assert_eq!(p.set_mask(0o002), Ok(0o077));
        assert_eq!(p.history(), &[0u16, 0o022, 0o077][..]);
    }

    #[test]
    fn duplicate_and_missing() {
        let mut p = Proc::new();
        assert_eq!(p.create(b"x", K_FILE, 0o644), Ok(0o644));
        assert_eq!(p.create(b"x", K_FILE, 0o644), Err(UM_EEXIST));
        assert_eq!(p.chmod(b"y", 0o644), Err(UM_ENOENT));
    }

    #[test]
    fn ledger_green() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert!(l.audit().is_ok());
        assert_eq!(inject_four(), [true; 4]);
        assert_eq!(run_unx11436_checks().failed_count(), 0);
    }
}
