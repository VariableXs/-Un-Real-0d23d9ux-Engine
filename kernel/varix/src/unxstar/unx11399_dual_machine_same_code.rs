//! UNX-F11399 三工具判据双机同码验证（0xeef0 段）
//!
//! 同一份判据必须在两台机器上跑出同一个结果：S1 与 M7。双机同码验证要抓的
//! 不是「两边都绿」，而是**两边绿得一模一样**——同一条用例、同一种比对方式、
//! 同一个结论。只要有一台机器对某条用例给出不同答案，就不是环境差异，而是
//! 判据本身依赖了不该依赖的东西。
//!
//! 比对分两档，档位不同，比法就不同：
//!
//! - **快照档**（`SNAP`）：输出被整份存成快照文件，两台机器必须**逐字节**
//!   相同。这里比的是内容摘要 `digest`，任何一字节漂移都判 `EBYTE`；
//! - **动态档**（`DYN`）：输出里含时间戳、临时路径、PID 等天然不同的列，
//!   因此先归一化（`norm`），再只比**关键列** `cols[0..ncols]`。关键列不同判
//!   `ECOL`；未归一化的动态行根本不该参与比对，判 `ENORM`。
//!
//! 由此派生的错误类彼此不可替代：
//!
//! - `ESHAPE`：用例编号为 0、机器身份非法、两机身份相同、或种类码越界。
//!   这是形状问题，连「该不该比」都没确定；
//! - `EKIND`：同一用例在两台机器上被记成不同档位（一边快照一边动态）。这说
//!   明用例定义本身分叉了，比内容没有意义；
//! - `EBYTE`：快照档摘要不等——字节漂移；
//! - `ECOL`：动态档关键列不等——语义漂移；
//! - `ENORM`：动态行未归一化。它是**前置条件缺失**，不是结果不一致；
//! - `EMISS`：某条用例只出现在一台机器上。缺行与行不同必须分开报，否则
//!   「一致率 100%」会被空集凑出来；
//! - `EDUP`：同一机器上同编号二次登记；
//! - `EFULL`：单机结果表满（8 条）。
//!
//! 闸门次序（`compare`）：机器身份 → 机器非空 → 逐行找对端 → 档位 → 快照
//! 摘要 / 动态关键列 → 对端多余行。次序即判据：身份不对时绝不先报缺行；
//! 档位不同时绝不先比字节。静态档逐字节、动态档关键列，一致率必须恰好
//! 100%，不做「四舍五入到 100」。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xeef0;

/// 形状错：编号为 0、机器身份非法或种类码越界。
pub const DV_ESHAPE: u32 = SEG_BASE + 0x01;
/// 同一用例在两机的档位不同。
pub const DV_EKIND: u32 = SEG_BASE + 0x02;
/// 快照档摘要不等（字节漂移）。
pub const DV_EBYTE: u32 = SEG_BASE + 0x03;
/// 动态档关键列不等（语义漂移）。
pub const DV_ECOL: u32 = SEG_BASE + 0x04;
/// 动态行未归一化。
pub const DV_ENORM: u32 = SEG_BASE + 0x05;
/// 用例只出现在一台机器上。
pub const DV_EMISS: u32 = SEG_BASE + 0x06;
/// 同机同编号重复登记。
pub const DV_EDUP: u32 = SEG_BASE + 0x07;
/// 单机结果表已满。
pub const DV_EFULL: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const DV_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const DV_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const DV_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const DV_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const DV_ENOBS: u32 = SEG_BASE + 0x0d;

/// 机器身份：S1。
pub const S1: u8 = 0;
/// 机器身份：M7。
pub const M7: u8 = 1;
/// 档位：快照档，逐字节。
pub const SNAP: u8 = 0;
/// 档位：动态档，只比关键列。
pub const DYN: u8 = 1;

/// 单机结果表容量（即 `CAP`）。
pub const CAP: usize = 8;
/// 动态档关键列数上限。
pub const NCOL: usize = 4;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 一致率目标。
pub const FULL: u32 = 100;

/// 操作码：登记一条结果。
pub const OP_PUSH: u8 = 1;
/// 操作码：双机比对。
pub const OP_COMPARE: u8 = 2;
/// 操作码：读一致率。
pub const OP_RATE: u8 = 3;
/// 操作码：读单机条数。
pub const OP_LEN: u8 = 4;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个 64 位字。
pub fn fold(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut n = v;
    let mut i = 0u32;
    while i < 8 {
        x ^= n & 0xff;
        x = x.wrapping_mul(FNV_PRIME);
        n >>= 8;
        i += 1;
    }
    x
}

/// 一条用例结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    /// 用例编号。
    pub id: u16,
    /// 档位：`SNAP` 或 `DYN`。
    pub kind: u8,
    /// 动态档是否已归一化；快照档无意义但按真填。
    pub norm: bool,
    /// 快照内容摘要。
    pub digest: u32,
    /// 动态档关键列。
    pub cols: [u32; NCOL],
    /// 有效关键列数。
    pub ncols: u8,
    /// 是否在册。
    pub live: bool,
}

impl Case {
    /// 空槽。
    pub const fn blank() -> Case {
        Case { id: 0, kind: 0, norm: false, digest: 0, cols: [0; NCOL], ncols: 0, live: false }
    }

    /// 关键列逐列相等（列数也要相等）。
    pub fn cols_eq(&self, other: &Case) -> bool {
        if self.ncols != other.ncols {
            return false;
        }
        let mut i = 0usize;
        while i < self.ncols as usize {
            let a = match self.cols.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let b = match other.cols.get(i) {
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

/// 一台机器的结果表。
#[derive(Clone, Copy, Debug)]
pub struct Machine {
    rows: [Case; CAP],
    n: usize,
    tag: u8,
}

impl Machine {
    /// 建一台机器。
    pub const fn new(tag: u8) -> Machine {
        Machine { rows: [Case::blank(); CAP], n: 0, tag }
    }

    /// 机器身份。
    pub fn tag(&self) -> u8 {
        self.tag
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 第 i 条。
    pub fn row(&self, i: usize) -> Option<Case> {
        if i >= self.n {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 按编号查找。
    pub fn find(&self, id: u16) -> Option<Case> {
        let mut i = 0usize;
        while i < self.n {
            if let Some(c) = self.rows.get(i) {
                if c.live && c.id == id {
                    return Some(*c);
                }
            }
            i += 1;
        }
        None
    }

    /// 登记一条结果。
    ///
    /// 闸门：机器身份 → 形状（编号/档位）→ 动态归一化 → 重复 → 容量。
    pub fn push(
        &mut self,
        id: u16,
        kind: u8,
        norm: bool,
        digest: u32,
        cols: [u32; NCOL],
        ncols: u8,
    ) -> Result<usize, u32> {
        if self.tag != S1 && self.tag != M7 {
            return Err(DV_ESHAPE);
        }
        if id == 0 || kind > DYN {
            return Err(DV_ESHAPE);
        }
        if kind == DYN && (!norm || ncols == 0 || ncols as usize > NCOL) {
            return Err(DV_ENORM);
        }
        if self.find(id).is_some() {
            return Err(DV_EDUP);
        }
        if self.n >= CAP {
            return Err(DV_EFULL);
        }
        if let Some(dst) = self.rows.get_mut(self.n) {
            *dst = Case { id, kind, norm, digest, cols, ncols, live: true };
        }
        self.n += 1;
        Ok(self.n)
    }

    /// 不变式：条数不越容量、编号互不相同、动态行都已归一化。
    pub fn consistent(&self) -> bool {
        if self.n > CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            let a = match self.rows.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if !a.live || a.id == 0 {
                return false;
            }
            if a.kind == DYN && !a.norm {
                return false;
            }
            if a.kind > DYN {
                return false;
            }
            let mut j = i + 1;
            while j < self.n {
                if let Some(b) = self.rows.get(j) {
                    if b.live && b.id == a.id {
                        return false;
                    }
                }
                j += 1;
            }
            i += 1;
        }
        true
    }
}

/// 双机比对。
///
/// 闸门：机器身份 → 非空 → 逐行找对端 → 档位 → 快照摘要 / 动态关键列 →
/// 对端多余行。返回一致条数；任何一处分歧立即返回对应的错误类。
pub fn compare(a: &Machine, b: &Machine) -> Result<usize, u32> {
    if a.tag != S1 || b.tag != M7 {
        return Err(DV_ESHAPE);
    }
    if a.is_empty() || b.is_empty() {
        return Err(DV_EMISS);
    }
    let mut agreed = 0usize;
    let mut i = 0usize;
    while i < a.len() {
        let x = match a.row(i) {
            Some(v) => v,
            None => return Err(DV_EMISS),
        };
        let y = match b.find(x.id) {
            Some(v) => v,
            None => return Err(DV_EMISS),
        };
        if x.kind != y.kind {
            return Err(DV_EKIND);
        }
        if x.kind == SNAP {
            if x.digest != y.digest {
                return Err(DV_EBYTE);
            }
        } else {
            if !x.norm || !y.norm {
                return Err(DV_ENORM);
            }
            if !x.cols_eq(&y) {
                return Err(DV_ECOL);
            }
        }
        agreed += 1;
        i += 1;
    }
    let mut j = 0usize;
    while j < b.len() {
        if let Some(y) = b.row(j) {
            if a.find(y.id).is_none() {
                return Err(DV_EMISS);
            }
        }
        j += 1;
    }
    Ok(agreed)
}

/// 一致率（百分比）。逐条比完才谈百分比，绝不先算率再看分歧。
pub fn agreement(a: &Machine, b: &Machine) -> Result<u32, u32> {
    if a.is_empty() || b.is_empty() {
        return Err(DV_EMISS);
    }
    let n = compare(a, b)?;
    if n != a.len() || n != b.len() {
        return Err(DV_EMISS);
    }
    let total = a.len() as u32;
    Ok((n as u32).saturating_mul(FULL) / total)
}

/// 一格观测。
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

/// `Result<usize, u32>` → 样本。
fn cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
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
            return Err(DV_ESLOT);
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

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(DV_EBAL);
        }
        const VALS: [u32; 8] = [1, 1, 2, 2, 2, 100, 3, 3];
        const CODES: [u32; 6] = [DV_ESHAPE, DV_EKIND, DV_EBYTE, DV_ECOL, DV_ENORM, DV_EMISS];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(DV_EBAL),
            };
            if s.slot as usize != i {
                return Err(DV_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(DV_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(DV_EVALUE);
                        }
                    }
                    None => return Err(DV_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(DV_EVALUE);
                        }
                    }
                    None => return Err(DV_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(DV_EBAL);
        }
        Ok(())
    }

    /// 指纹（只折内容行）。
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
        h
    }

    /// 冻结并记根。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(DV_EBAL);
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
            Err(DV_ESEAL)
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

/// 六条用例的对照组：三条快照、三条动态，双机内容完全一致。
pub fn twin_pair() -> (Machine, Machine) {
    let mut a = Machine::new(S1);
    let mut b = Machine::new(M7);
    let _ = a.push(10, SNAP, false, 0xa0, [0; NCOL], 0);
    let _ = b.push(10, SNAP, false, 0xa0, [0; NCOL], 0);
    let _ = a.push(11, DYN, true, 0, [7, 8, 0, 0], 2);
    let _ = b.push(11, DYN, true, 0, [7, 8, 0, 0], 2);
    let _ = a.push(12, SNAP, false, 0xb0, [0; NCOL], 0);
    let _ = b.push(12, SNAP, false, 0xb0, [0; NCOL], 0);
    let _ = a.push(13, DYN, true, 0, [1, 2, 3, 0], 3);
    let _ = b.push(13, DYN, true, 0, [1, 2, 3, 0], 3);
    let _ = a.push(14, SNAP, false, 0xc0, [0; NCOL], 0);
    let _ = b.push(14, SNAP, false, 0xc0, [0; NCOL], 0);
    let _ = a.push(15, DYN, true, 0, [4, 5, 6, 7], 4);
    let _ = b.push(15, DYN, true, 0, [4, 5, 6, 7], 4);
    (a, b)
}

/// 十四格账本：四条登记 + 比对 + 一致率 + 两条登记 + 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut s1 = Machine::new(S1);
    let mut m7 = Machine::new(M7);

    let _ = l.register(cell(0, OP_PUSH, s1.push(10, SNAP, false, 0xa0, [0; NCOL], 0)));
    let _ = l.register(cell(1, OP_PUSH, m7.push(10, SNAP, false, 0xa0, [0; NCOL], 0)));
    let _ = l.register(cell(2, OP_PUSH, s1.push(11, DYN, true, 0, [7, 8, 0, 0], 2)));
    let _ = l.register(cell(3, OP_PUSH, m7.push(11, DYN, true, 0, [7, 8, 0, 0], 2)));
    let _ = l.register(cell(4, OP_COMPARE, compare(&s1, &m7)));
    let _ = l.register(word_cell(5, OP_RATE, agreement(&s1, &m7)));
    let _ = l.register(cell(6, OP_PUSH, s1.push(12, SNAP, false, 0xb0, [0; NCOL], 0)));
    let _ = l.register(cell(7, OP_PUSH, m7.push(12, SNAP, false, 0xb0, [0; NCOL], 0)));

    let mut a = Machine::new(S1);
    let _ = l.register(cell(8, OP_PUSH, a.push(0, SNAP, false, 1, [0; NCOL], 0)));

    let mut x = Machine::new(S1);
    let mut y = Machine::new(M7);
    let _ = x.push(20, SNAP, false, 0xd0, [0; NCOL], 0);
    let _ = y.push(20, DYN, true, 0, [1, 2, 3, 0], 3);
    let _ = l.register(cell(9, OP_COMPARE, compare(&x, &y)));

    let mut b1 = Machine::new(S1);
    let mut b2 = Machine::new(M7);
    let _ = b1.push(21, SNAP, false, 0xe1, [0; NCOL], 0);
    let _ = b2.push(21, SNAP, false, 0xe2, [0; NCOL], 0);
    let _ = l.register(cell(10, OP_COMPARE, compare(&b1, &b2)));

    let mut c1 = Machine::new(S1);
    let mut c2 = Machine::new(M7);
    let _ = c1.push(22, DYN, true, 0, [3, 4, 9, 0], 3);
    let _ = c2.push(22, DYN, true, 0, [3, 4, 8, 0], 3);
    let _ = l.register(cell(11, OP_COMPARE, compare(&c1, &c2)));

    let mut d = Machine::new(S1);
    let _ = l.register(cell(12, OP_PUSH, d.push(23, DYN, false, 0, [1, 0, 0, 0], 1)));

    let mut e1 = Machine::new(S1);
    let e2 = Machine::new(M7);
    let _ = e1.push(24, SNAP, false, 0xf0, [0; NCOL], 0);
    let _ = l.register(cell(13, OP_COMPARE, compare(&e1, &e2)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    // 一路：账本形状、对账、重放、指纹。
    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES
        && l.ok_count() == 8
        && l.err_count() == 6
        && l.audit().is_ok()
        && rep == Ok(root)
        && root != 0;

    // 二路：三快照三动态，双机逐条一致，一致率恰好 100。
    let (a, b) = twin_pair();
    out[1] = a.len() == 6
        && b.len() == 6
        && compare(&a, &b) == Ok(6)
        && agreement(&a, &b) == Ok(FULL)
        && a.find(10).map(|c| c.digest) == b.find(10).map(|c| c.digest)
        && a.find(15).map(|c| c.cols) == b.find(15).map(|c| c.cols)
        && a.consistent()
        && b.consistent();

    // 三路：六类分歧彼此可辨。
    let mut p = Machine::new(S1);
    let shape = p.push(0, SNAP, false, 1, [0; NCOL], 0) == Err(DV_ESHAPE);

    let mut k1 = Machine::new(S1);
    let mut k2 = Machine::new(M7);
    let _ = k1.push(30, SNAP, false, 1, [0; NCOL], 0);
    let _ = k2.push(30, DYN, true, 0, [1, 2, 0, 0], 2);
    let kind = compare(&k1, &k2) == Err(DV_EKIND);

    let mut y1 = Machine::new(S1);
    let mut y2 = Machine::new(M7);
    let _ = y1.push(31, SNAP, false, 0x11, [0; NCOL], 0);
    let _ = y2.push(31, SNAP, false, 0x12, [0; NCOL], 0);
    let byte = compare(&y1, &y2) == Err(DV_EBYTE);

    let mut c1 = Machine::new(S1);
    let mut c2 = Machine::new(M7);
    let _ = c1.push(32, DYN, true, 0, [5, 6, 0, 0], 2);
    let _ = c2.push(32, DYN, true, 0, [5, 7, 0, 0], 2);
    let col = compare(&c1, &c2) == Err(DV_ECOL);

    let mut nd = Machine::new(S1);
    let norm = nd.push(33, DYN, false, 0, [1, 0, 0, 0], 1) == Err(DV_ENORM);

    let mut m1 = Machine::new(S1);
    let m2 = Machine::new(M7);
    let _ = m1.push(34, SNAP, false, 1, [0; NCOL], 0);
    let miss = compare(&m1, &m2) == Err(DV_EMISS);
    out[2] = shape && kind && byte && col && norm && miss && p.consistent();

    // 四路：闸门次序、机器身份与容量边界。
    let mut q = Machine::new(S1);
    let g1 = q.push(0, 7, false, 0, [0; NCOL], 0) == Err(DV_ESHAPE);
    let g2 = q.push(1, 7, false, 0, [0; NCOL], 0) == Err(DV_ESHAPE);
    let g3 = q.push(2, DYN, false, 0, [1, 0, 0, 0], 1) == Err(DV_ENORM);
    let mut wrong = Machine::new(9);
    let g4 = wrong.push(3, SNAP, false, 1, [0; NCOL], 0) == Err(DV_ESHAPE);
    let g5 = compare(&q, &q) == Err(DV_ESHAPE);
    let g6 = agreement(&q, &Machine::new(M7)) == Err(DV_EMISS);
    let mut full = Machine::new(S1);
    let mut i = 1u16;
    while full.len() < CAP {
        let _ = full.push(i, SNAP, false, i as u32, [0; NCOL], 0);
        i += 1;
    }
    let g7 = full.push(99, SNAP, false, 9, [0; NCOL], 0) == Err(DV_EFULL);
    let g8 = full.push(1, SNAP, false, 9, [0; NCOL], 0) == Err(DV_EDUP);
    out[3] = g1 && g2 && g3 && g4 && g5 && g6 && g7 && g8 && q.consistent() && full.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11399_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11399.dual_machine_same_code");
    let l = build_ledger();

    s.add_dyn(
        "DV1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "DV2-快照逐字节相同",
        l.sample(0).map(|x| x.value) == Some(1)
            && l.sample(1).map(|x| x.value) == Some(1)
            && {
                let (a, b) = twin_pair();
                a.find(10).map(|c| c.digest) == b.find(10).map(|c| c.digest)
                    && a.find(12).map(|c| c.digest) == b.find(12).map(|c| c.digest)
                    && compare(&a, &b) == Ok(6)
            },
        "static",
    );

    s.add(
        "DV3-动态关键列相同",
        l.sample(2).map(|x| x.value) == Some(2)
            && l.sample(3).map(|x| x.value) == Some(2)
            && {
                let (a, b) = twin_pair();
                a.find(13).map(|c| c.cols_eq(&match b.find(13) {
                    Some(v) => v,
                    None => Case::blank(),
                })) == Some(true)
                    && a.find(15).map(|c| c.ncols) == Some(4)
            },
        "static",
    );

    s.add(
        "DV4-一致率恰好100",
        l.sample(4).map(|x| x.value) == Some(2)
            && l.sample(5).map(|x| x.value) == Some(FULL)
            && {
                let (a, b) = twin_pair();
                agreement(&a, &b) == Ok(100)
                    && {
                        let mut c = Machine::new(M7);
                        let _ = c.push(10, SNAP, false, 0xa0, [0; NCOL], 0);
                        agreement(&c, &a) == Err(DV_ESHAPE)
                    }
            },
        "static",
    );

    s.add(
        "DV5-档位分叉EKIND",
        l.sample(9).map(|x| x.code) == Some(DV_EKIND)
            && {
                let mut a = Machine::new(S1);
                let mut b = Machine::new(M7);
                let _ = a.push(40, SNAP, false, 1, [0; NCOL], 0);
                let _ = b.push(40, DYN, true, 0, [1, 0, 0, 0], 1);
                compare(&a, &b) == Err(DV_EKIND)
            },
        "static",
    );

    s.add(
        "DV6-单边缺行EMISS",
        l.sample(13).map(|x| x.code) == Some(DV_EMISS)
            && {
                let mut a = Machine::new(S1);
                let mut b = Machine::new(M7);
                let _ = a.push(41, SNAP, false, 1, [0; NCOL], 0);
                let _ = b.push(42, SNAP, false, 2, [0; NCOL], 0);
                let forward = compare(&a, &b) == Err(DV_EMISS);
                let _ = b.push(41, SNAP, false, 1, [0; NCOL], 0);
                forward && compare(&a, &b) == Err(DV_EMISS)
            },
        "static",
    );

    s.add(
        "DV7-形状坏ESHAPE",
        l.sample(8).map(|x| x.code) == Some(DV_ESHAPE)
            && {
                let mut a = Machine::new(S1);
                let bad_id = a.push(0, SNAP, false, 1, [0; NCOL], 0) == Err(DV_ESHAPE);
                let bad_kind = a.push(1, 9, false, 1, [0; NCOL], 0) == Err(DV_ESHAPE);
                let mut bad_tag = Machine::new(5);
                let bad_machine = bad_tag.push(1, SNAP, false, 1, [0; NCOL], 0) == Err(DV_ESHAPE);
                bad_id && bad_kind && bad_machine && a.len() == 0
            },
        "static",
    );

    s.add(
        "DV8-未归一化ENORM",
        l.sample(12).map(|x| x.code) == Some(DV_ENORM)
            && {
                let mut a = Machine::new(S1);
                a.push(43, DYN, false, 0, [1, 2, 0, 0], 2) == Err(DV_ENORM)
                    && a.push(44, DYN, true, 0, [1, 2, 0, 0], 0) == Err(DV_ENORM)
            },
        "static",
    );

    s.add(
        "DV9-重复登记EDUP",
        {
            let mut a = Machine::new(S1);
            let one = a.push(45, SNAP, false, 1, [0; NCOL], 0) == Ok(1);
            let two = a.push(45, SNAP, false, 2, [0; NCOL], 0) == Err(DV_EDUP);
            one && two && a.len() == 1 && a.find(45).map(|c| c.digest) == Some(1)
        },
        "static",
    );

    s.add(
        "DV10-单机满EFULL",
        {
            let mut a = Machine::new(S1);
            let mut i = 1u16;
            while a.len() < CAP {
                let _ = a.push(i, SNAP, false, i as u32, [0; NCOL], 0);
                i += 1;
            }
            a.push(99, SNAP, false, 9, [0; NCOL], 0) == Err(DV_EFULL)
                && a.len() == CAP
                && a.consistent()
        },
        "static",
    );

    s.add(
        "DV11-三快照三动态全对",
        {
            let (a, b) = twin_pair();
            let mut snaps = 0usize;
            let mut dyns = 0usize;
            let mut i = 0usize;
            while i < a.len() {
                if let Some(c) = a.row(i) {
                    if c.kind == SNAP {
                        snaps += 1;
                    } else {
                        dyns += 1;
                    }
                }
                i += 1;
            }
            snaps == 3 && dyns == 3 && compare(&a, &b) == Ok(6) && agreement(&a, &b) == Ok(FULL)
        },
        "static",
    );

    s.add(
        "DV12-机器身份S1对M7",
        {
            let (a, b) = twin_pair();
            let ok = compare(&a, &b) == Ok(6);
            let swapped = compare(&b, &a) == Err(DV_ESHAPE);
            let same = compare(&a, &a) == Err(DV_ESHAPE);
            let empty = agreement(&Machine::new(S1), &Machine::new(M7)) == Err(DV_EMISS);
            ok && swapped && same && empty && a.tag() == S1 && b.tag() == M7
        },
        "static",
    );

    s.add(
        "DV13-冻结重放四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two.is_err()
                && f.rejects() == 1
                && f.is_frozen()
                && f.root() != 0
                && f.root() == f.seal()
                && inj == [true; 4]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11399_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn twins_agree_fully() {
        let (a, b) = twin_pair();
        assert_eq!(a.len(), 6);
        assert_eq!(b.len(), 6);
        assert_eq!(compare(&a, &b), Ok(6));
        assert_eq!(agreement(&a, &b), Ok(100));
        assert!(a.consistent() && b.consistent());
    }

    #[test]
    fn byte_drift_is_detected() {
        let mut a = Machine::new(S1);
        let mut b = Machine::new(M7);
        let _ = a.push(1, SNAP, false, 0x10, [0; NCOL], 0);
        let _ = b.push(1, SNAP, false, 0x11, [0; NCOL], 0);
        assert_eq!(compare(&a, &b), Err(DV_EBYTE));
    }

    #[test]
    fn column_drift_is_detected() {
        let mut a = Machine::new(S1);
        let mut b = Machine::new(M7);
        let _ = a.push(1, DYN, true, 0, [1, 0, 0, 0], 1);
        let _ = b.push(1, DYN, true, 0, [2, 0, 0, 0], 1);
        assert_eq!(compare(&a, &b), Err(DV_ECOL));
    }

    #[test]
    fn kinds_must_match() {
        let mut a = Machine::new(S1);
        let mut b = Machine::new(M7);
        let _ = a.push(1, SNAP, false, 1, [0; NCOL], 0);
        let _ = b.push(1, DYN, true, 0, [1, 0, 0, 0], 1);
        assert_eq!(compare(&a, &b), Err(DV_EKIND));
    }

    #[test]
    fn missing_rows_are_not_agreement() {
        let mut a = Machine::new(S1);
        let mut b = Machine::new(M7);
        let _ = a.push(1, SNAP, false, 1, [0; NCOL], 0);
        assert_eq!(compare(&a, &b), Err(DV_EMISS));
        let _ = b.push(1, SNAP, false, 1, [0; NCOL], 0);
        assert_eq!(compare(&a, &b), Ok(1));
    }

    #[test]
    fn inject_four_is_all_true() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
