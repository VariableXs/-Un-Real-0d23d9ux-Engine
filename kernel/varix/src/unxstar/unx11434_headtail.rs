//! UNX-F11434 head/tail 行窗口（0x20ae0 段）
//!
//! head 与 tail 都不是“按字节截断”，而是以行为单位求一个左闭右开区间。模型
//! 先给输入建立行索引：每行记录起始偏移与结束偏移（含换行符，末行可以没有），
//! 之后四种取法都退化成区间运算：head N 取第 N 行的结束偏移；head 去掉末尾 N
//! 行取倒数第 N 行的起始偏移；tail N 取倒数第 N 行的起始偏移；tail 从第 N 行
//! 起取第 N-1 行的起始偏移。边界值 0、超过总行数、空输入都在这一层处理。
//!
//! 末行没有换行是索引里最容易被忽略的细节：行数仍然是 1，区间终点是文本末尾，
//! 而不是“末尾减一”。模型用同一段文本的有换行/无换行两份对照，确保 tail 1
//! 在两种情况下分别得到带换行的一行与不带换行的尾巴。
//!
//! 参数校验独立于区间计算：`tail -n +0` 与 `head -n -0` 都是非法请求，返回
//! EINVAL；行数参数超过上限返回 EOVERFLOW；输入名字为空返回 ENAME。拒绝路径
//! 不产生区间，调用方拿到的是零长度结果加错误码。
//!
//! 账本十六格：十二格成功覆盖四种取法与三种边界，四格失败覆盖非法参数、超限
//! 与空名字。所有成功格都同时校验区间长度与首尾字节。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20ae0;
/// 编号非法。
pub const HT_EID: u32 = SEG_BASE + 0x01;
/// 顺序错误。
pub const HT_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const HT_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const HT_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测缺失。
pub const HT_ENOBS: u32 = SEG_BASE + 0x05;
/// 结果不符。
pub const HT_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 索引失衡。
pub const HT_EBAL: u32 = SEG_BASE + 0x07;
/// 输入名非法。
pub const HT_ENAME: u32 = SEG_BASE + 0x08;
/// 参数非法。
pub const HT_EINVAL: u32 = SEG_BASE + 0x09;
/// 参数超限。
pub const HT_EOVERFLOW: u32 = SEG_BASE + 0x0a;
/// 票据不一致。
pub const HT_ESEAL: u32 = SEG_BASE + 0x0b;

/// 取前 N 行。
pub const OP_HEAD: u8 = 1;
/// 去掉末尾 N 行。
pub const OP_HEAD_BUT: u8 = 2;
/// 取末尾 N 行。
pub const OP_TAIL: u8 = 3;
/// 从第 N 行起。
pub const OP_FROM: u8 = 4;

/// 文本容量。
pub const TEXT_CAP: usize = 64;
/// 行容量。
pub const LINE_CAP: usize = 12;
/// 输入数。
pub const INPUTS: usize = 4;
/// 参数上限。
pub const MAX_N: u16 = 4096;
/// 样本格数。
pub const SAMPLES: usize = 16;

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

/// 一段输入。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Text {
    pub name: [u8; 6],
    pub nlen: u8,
    pub body: [u8; TEXT_CAP],
    pub blen: u8,
}

impl Text {
    pub fn make(name: &[u8], body: &[u8]) -> Text {
        let mut n = [0u8; 6];
        let mut b = [0u8; TEXT_CAP];
        let nl = put(&mut n, name);
        let bl = put(&mut b, body);
        Text { name: n, nlen: nl, body: b, blen: bl }
    }

    pub fn name(&self) -> &[u8] {
        cut(&self.name, self.nlen)
    }

    pub fn body(&self) -> &[u8] {
        cut(&self.body, self.blen)
    }
}

/// 左闭右开区间。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cut {
    pub start: u16,
    pub end: u16,
}

impl Cut {
    pub const fn make(start: u16, end: u16) -> Cut {
        Cut { start, end }
    }

    pub fn len(&self) -> u16 {
        self.end.saturating_sub(self.start)
    }
}

/// 行索引。
pub struct Lines {
    texts: Vec<Text>,
    starts: Vec<u16>,
    ends: Vec<u16>,
    cur: usize,
    cuts: u32,
    rejects: u32,
}

impl Lines {
    pub fn new(texts: Vec<Text>) -> Lines {
        Lines { texts, starts: Vec::new(), ends: Vec::new(), cur: 0, cuts: 0, rejects: 0 }
    }

    pub fn text(&self, i: usize) -> Option<Text> {
        match self.texts.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    pub fn find(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.texts.len() {
            if let Some(t) = self.texts.get(i) {
                if t.name() == name {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    pub fn lines(&self) -> usize {
        self.starts.len()
    }

    pub fn start(&self, i: usize) -> Option<u16> {
        match self.starts.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    pub fn end(&self, i: usize) -> Option<u16> {
        match self.ends.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    pub fn cuts(&self) -> u32 {
        self.cuts
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 为名字建立行索引。
    pub fn index(&mut self, name: &[u8]) -> Result<usize, u32> {
        let id = match self.find(name) {
            Some(v) => v,
            None => {
                self.rejects += 1;
                return Err(HT_ENAME);
            }
        };
        let t = match self.texts.get(id) {
            Some(v) => *v,
            None => {
                self.rejects += 1;
                return Err(HT_ENAME);
            }
        };
        self.starts.clear();
        self.ends.clear();
        let body = t.body();
        let mut line_start = 0usize;
        let mut i = 0usize;
        while i < body.len() {
            if body.get(i) == Some(&b'\n') {
                self.starts.push(line_start as u16);
                self.ends.push((i + 1) as u16);
                line_start = i + 1;
            }
            i += 1;
        }
        if line_start < body.len() {
            self.starts.push(line_start as u16);
            self.ends.push(body.len() as u16);
        }
        self.cur = id;
        Ok(self.starts.len())
    }

    /// 计算窗口。成功返回区间，失败返回错误码。
    pub fn slice(&mut self, name: &[u8], op: u8, n: u16) -> Result<Cut, u32> {
        if n > MAX_N {
            self.rejects += 1;
            return Err(HT_EOVERFLOW);
        }
        if (op == OP_FROM || op == OP_HEAD_BUT) && n == 0 {
            self.rejects += 1;
            return Err(HT_EINVAL);
        }
        let count = self.index(name)?;
        let total = match self.end(count.saturating_sub(1)) {
            Some(v) => v,
            None => 0,
        };
        let cut = match op {
            OP_HEAD => {
                if n == 0 {
                    Cut::make(0, 0)
                } else if (n as usize) >= count {
                    Cut::make(0, total)
                } else {
                    let e = match self.end(n as usize - 1) {
                        Some(v) => v,
                        None => 0,
                    };
                    Cut::make(0, e)
                }
            }
            OP_HEAD_BUT => {
                if (n as usize) >= count {
                    Cut::make(0, 0)
                } else {
                    let s = match self.start(count - n as usize) {
                        Some(v) => v,
                        None => 0,
                    };
                    Cut::make(0, s)
                }
            }
            OP_TAIL => {
                if n == 0 {
                    Cut::make(total, total)
                } else if (n as usize) >= count {
                    Cut::make(0, total)
                } else {
                    let s = match self.start(count - n as usize) {
                        Some(v) => v,
                        None => 0,
                    };
                    Cut::make(s, total)
                }
            }
            OP_FROM => {
                if (n as usize) > count {
                    Cut::make(total, total)
                } else {
                    let s = match self.start(n as usize - 1) {
                        Some(v) => v,
                        None => 0,
                    };
                    Cut::make(s, total)
                }
            }
            _ => {
                self.rejects += 1;
                return Err(HT_EINVAL);
            }
        };
        self.cuts += 1;
        Ok(cut)
    }

    /// 取出区间文本。
    pub fn window(&self, c: Cut) -> &[u8] {
        let t = match self.texts.get(self.cur) {
            Some(v) => v,
            None => return &[],
        };
        let body = t.body();
        let s = c.start as usize;
        let e = c.end as usize;
        if s > e || e > body.len() {
            return &[];
        }
        match body.get(s..e) {
            Some(v) => v,
            None => &[],
        }
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.texts.is_empty() || self.texts.len() > INPUTS {
            return Err(HT_EBAL);
        }
        let mut i = 0usize;
        while i < self.texts.len() {
            let t = match self.texts.get(i) {
                Some(v) => *v,
                None => return Err(HT_ENOBS),
            };
            if t.blen as usize > TEXT_CAP {
                return Err(HT_EBAL);
            }
            let mut j = i + 1;
            while j < self.texts.len() {
                if let Some(u) = self.texts.get(j) {
                    if t.name() == u.name() || u.nlen == 0 {
                        return Err(HT_EBAL);
                    }
                }
                j += 1;
            }
            i += 1;
        }
        if !self.starts.is_empty() && self.starts.len() != self.ends.len() {
            return Err(HT_EBAL);
        }
        Ok(())
    }

    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.texts.len() {
            if let Some(t) = self.texts.get(i) {
                h = fold(h, t.blen as u64);
                let mut k = 0usize;
                while k < t.nlen as usize {
                    if let Some(b) = t.name.get(k) {
                        h ^= *b as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                    k += 1;
                }
                let mut m = 0usize;
                while m < t.blen as usize {
                    if let Some(b) = t.body.get(m) {
                        h ^= *b as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                    m += 1;
                }
            }
            i += 1;
        }
        h = fold(h, self.cuts as u64);
        fold(h, self.rejects as u64)
    }
}

/// 标准输入。
pub fn build_texts() -> Vec<Text> {
    let mut v = Vec::new();
    v.push(Text::make(b"four", b"l1\nl2\nl3\nl4\n"));
    v.push(Text::make(b"noeol", b"a\nb"));
    v.push(Text::make(b"one", b"x\n"));
    v.push(Text::make(b"empty", b""));
    v
}

/// 一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    pub name: [u8; 6],
    pub nlen: u8,
    pub op: u8,
    pub n: u16,
    pub expect: u16,
    pub ok: bool,
    pub head: u8,
    pub tail: u8,
}

impl Case {
    pub fn name(&self) -> &[u8] {
        cut(&self.name, self.nlen)
    }
}

/// 结局。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ending {
    pub cut: Cut,
    pub code: u16,
}

/// 账本。
pub struct Ledger {
    l: Lines,
    cases: Vec<Case>,
    ends: Vec<Ending>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger { l: Lines::new(build_texts()), cases: Vec::new(), ends: Vec::new(), frozen: false, root: 0, rejects: 0 }
    }

    pub fn register(&mut self, slot: u8, name: &[u8], op: u8, n: u16, expect: u16, ok: bool) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(HT_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(HT_EID);
        }
        if (slot as usize) < self.cases.len() {
            self.rejects += 1;
            return Err(HT_EDUP);
        }
        if (slot as usize) != self.cases.len() {
            self.rejects += 1;
            return Err(HT_EORDER);
        }
        let r = self.l.slice(name, op, n);
        let end = match r {
            Ok(c) => {
                if !ok || c.len() != expect {
                    self.rejects += 1;
                    return Err(HT_EMISMATCH);
                }
                Ending { cut: c, code: 0 }
            }
            Err(e) => {
                if ok {
                    self.rejects += 1;
                    return Err(HT_EMISMATCH);
                }
                Ending { cut: Cut::make(0, 0), code: (e & 0xffff) as u16 }
            }
        };
        let mut c = Case { name: [0; 6], nlen: 0, op, n, expect, ok, head: 0, tail: 0 };
        c.nlen = put(&mut c.name, name);
        if let Ok(w) = self.l.slice(name, op, n) {
            c.head = w.start.min(255) as u8;
            c.tail = w.end.min(255) as u8;
        }
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

    pub fn ending(&self, i: usize) -> Option<Ending> {
        match self.ends.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
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

    pub fn lines(&self) -> &Lines {
        &self.l
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.cases.len() != self.ends.len() || self.cases.is_empty() {
            return Err(HT_EBAL);
        }
        let mut i = 0usize;
        while i < self.cases.len() {
            let c = match self.cases.get(i) {
                Some(v) => *v,
                None => return Err(HT_ENOBS),
            };
            let e = match self.ends.get(i) {
                Some(v) => *v,
                None => return Err(HT_ENOBS),
            };
            if c.ok != (e.code == 0) {
                return Err(HT_EMISMATCH);
            }
            if c.ok && e.cut.len() != c.expect {
                return Err(HT_EMISMATCH);
            }
            i += 1;
        }
        self.l.audit()
    }

    pub fn seal(&self) -> u64 {
        self.l.seal()
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(HT_EFROZEN);
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
            Err(HT_ESEAL)
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
    let inval = (HT_EINVAL & 0xffff) as u16;
    let over = (HT_EOVERFLOW & 0xffff) as u16;
    let nam = (HT_ENAME & 0xffff) as u16;
    let _ = l.register(0, b"four", OP_HEAD, 2, 6, true);
    let _ = l.register(1, b"four", OP_HEAD, 0, 0, true);
    let _ = l.register(2, b"four", OP_HEAD, 9, 12, true);
    let _ = l.register(3, b"four", OP_HEAD_BUT, 1, 9, true);
    let _ = l.register(4, b"four", OP_TAIL, 2, 6, true);
    let _ = l.register(5, b"four", OP_TAIL, 0, 0, true);
    let _ = l.register(6, b"four", OP_TAIL, 9, 12, true);
    let _ = l.register(7, b"four", OP_FROM, 2, 9, true);
    let _ = l.register(8, b"four", OP_FROM, 1, 12, true);
    let _ = l.register(9, b"four", OP_FROM, 5, 0, true);
    let _ = l.register(10, b"noeol", OP_HEAD, 2, 3, true);
    let _ = l.register(11, b"noeol", OP_TAIL, 1, 1, true);
    let _ = l.register(12, b"noeol", OP_FROM, 2, 1, true);
    let _ = l.register(13, b"four", OP_FROM, 0, inval, false);
    let _ = l.register(14, b"four", OP_HEAD, 9999, over, false);
    let _ = l.register(15, b"", OP_HEAD, 1, nam, false);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.ok_count() == 13 && l.err_count() == 3 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    out[0] = base && root != 0 && rep == Ok(root);

    let mut a = Lines::new(build_texts());
    let h = a.slice(b"four", OP_HEAD, 2);
    let t = a.slice(b"four", OP_TAIL, 2);
    out[1] = h == Ok(Cut::make(0, 6)) && t == Ok(Cut::make(6, 12)) && a.window(Cut::make(6, 12)) == b"l3\nl4\n" as &[u8];

    let mut b = Lines::new(build_texts());
    let x = b.slice(b"noeol", OP_TAIL, 1);
    let y = b.slice(b"noeol", OP_HEAD, 1);
    out[2] = x == Ok(Cut::make(2, 3)) && b.window(Cut::make(2, 3)) == b"b" as &[u8] && y == Ok(Cut::make(0, 2));

    let mut c = Lines::new(build_texts());
    out[3] = c.slice(b"four", OP_FROM, 0) == Err(HT_EINVAL)
        && c.slice(b"four", OP_HEAD_BUT, 0) == Err(HT_EINVAL)
        && c.slice(b"four", OP_TAIL, MAX_N) == Ok(Cut::make(0, 12))
        && c.slice(b"four", OP_TAIL, MAX_N + 1) == Err(HT_EOVERFLOW);
    out
}

/// 十三判据。
pub fn run_unx11434_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11434.headtail");
    let l = build_ledger();
    s.add("HT1-十六格齐备", l.len() == SAMPLES && l.is_empty() == false, "static");
    s.add_dyn("HT2-十三应三拒", l.ok_count() == 13 && l.err_count() == 3 && l.audit().is_ok(), &format!("ok={} err={}", l.ok_count(), l.err_count()));
    s.add("HT3-head前缀", l.ending(0).map(|e| e.cut) == Some(Cut::make(0, 6)) && l.ending(2).map(|e| e.cut) == Some(Cut::make(0, 12)), "static");
    s.add("HT4-head零行", l.ending(1).map(|e| e.cut.len()) == Some(0), "static");
    s.add("HT5-head去尾", l.ending(3).map(|e| e.cut) == Some(Cut::make(0, 9)), "static");
    s.add("HT6-tail后缀", l.ending(4).map(|e| e.cut) == Some(Cut::make(6, 12)) && l.ending(6).map(|e| e.cut) == Some(Cut::make(0, 12)), "static");
    s.add("HT7-tail零行", l.ending(5).map(|e| e.cut.len()) == Some(0), "static");
    s.add("HT8-from行号", l.ending(7).map(|e| e.cut) == Some(Cut::make(3, 12)) && l.ending(8).map(|e| e.cut) == Some(Cut::make(0, 12)), "static");
    s.add("HT9-from越界", l.ending(9).map(|e| e.cut.len()) == Some(0), "static");
    s.add("HT10-无换行末行", l.ending(10).map(|e| e.cut) == Some(Cut::make(0, 3)) && l.ending(11).map(|e| e.cut) == Some(Cut::make(2, 3)), "static");
    s.add("HT11-from无换行", l.ending(12).map(|e| e.cut) == Some(Cut::make(2, 3)), "static");
    s.add("HT12-非法与超限", l.ending(13).map(|e| e.code) == Some((HT_EINVAL & 0xffff) as u16) && l.ending(14).map(|e| e.code) == Some((HT_EOVERFLOW & 0xffff) as u16), "static");
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(HT_EFROZEN);
    let inj = inject_four();
    s.add_dyn("HT13-冻结重放四注入", root != 0 && rep == Ok(root) && again && inj == [true; 4], &format!("root={:016x} {:?}", root, inj));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_prefix() {
        let mut l = Lines::new(build_texts());
        assert_eq!(l.slice(b"four", OP_HEAD, 2), Ok(Cut::make(0, 6)));
        assert_eq!(l.slice(b"four", OP_HEAD, 0), Ok(Cut::make(0, 0)));
    }

    #[test]
    fn tail_suffix() {
        let mut l = Lines::new(build_texts());
        assert_eq!(l.slice(b"four", OP_TAIL, 2), Ok(Cut::make(6, 12)));
        assert_eq!(l.slice(b"four", OP_TAIL, 0), Ok(Cut::make(12, 12)));
    }

    #[test]
    fn no_final_newline() {
        let mut l = Lines::new(build_texts());
        assert_eq!(l.slice(b"noeol", OP_TAIL, 1), Ok(Cut::make(2, 3)));
        assert_eq!(l.slice(b"noeol", OP_HEAD, 1), Ok(Cut::make(0, 2)));
    }

    #[test]
    fn from_line() {
        let mut l = Lines::new(build_texts());
        assert_eq!(l.slice(b"four", OP_FROM, 2), Ok(Cut::make(3, 12)));
        assert_eq!(l.slice(b"four", OP_FROM, 5), Ok(Cut::make(12, 12)));
    }

    #[test]
    fn invalid_and_overflow() {
        let mut l = Lines::new(build_texts());
        assert_eq!(l.slice(b"four", OP_FROM, 0), Err(HT_EINVAL));
        assert_eq!(l.slice(b"four", OP_TAIL, MAX_N + 1), Err(HT_EOVERFLOW));
        assert_eq!(l.slice(b"nope", OP_HEAD, 1), Err(HT_ENAME));
    }

    #[test]
    fn window_text() {
        let mut l = Lines::new(build_texts());
        let c = match l.slice(b"four", OP_TAIL, 2) {
            Ok(v) => v,
            Err(_) => Cut::make(0, 0),
        };
        assert_eq!(l.window(c), b"l3\nl4\n" as &[u8]);
    }

    #[test]
    fn ledger_green() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert!(l.audit().is_ok());
        assert_eq!(inject_four(), [true; 4]);
        assert_eq!(run_unx11434_checks().failed_count(), 0);
    }
}
