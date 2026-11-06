//! UNX-F11345 /proc/self/environ NUL 分隔语义（0xeb90 段）
//!
//! `/proc/self/environ` 是**环境块字节流**，不是文本文件：
//!
//! ```text
//! PATH=/bin\0HOME=/root\0TERM=vt100\0
//! ```
//!
//! 三条性质必须同时成立：
//!
//! 1. 条目之间用 NUL 分隔，**最后一条也有结尾 NUL**。按行读、按空格切，
//!    都会把多条环境变量粘成一条；
//! 2. 每条是 `NAME=VALUE`，`=` 右侧可以再出现 `=`（值里合法），左侧第一个
//!    `=` 才是分隔点；名字为空（`=x`）或整条为空都不是合法条目；
//! 3. 内核里的环境字符串是 C 字符串，内部不可能有 NUL。出现内嵌 NUL 说明
//!    调用方给的是被破坏的块，必须在构造阶段拒绝，而不是让消费者读到半条。
//!
//! 此外，environ 是**有序且允许重复键**的原始块；但作为可查询模型，重复键
//! 会让「取到哪一个」依赖扫描顺序。本模块在构造时拒绝重复键，让查询结果
//! 与顺序无关；顺序本身仍按输入保留，供逐条遍历。
//!
//! 闸门次序（构造）：条目数 → 空条目 → 缺等号 → 空名 → 内嵌 NUL →
//! 名字/值长度 → 重复键。前三项是 O(1)/O(n) 的形态检查；NUL 扫描覆盖
//! 整条；长度与重复键最后做，避免对已判定非法的条目继续比较。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xeb90;

/// 条目缺少等号。
pub const EV_ENOEQ: u32 = SEG_BASE + 0x01;
/// 名字为空（条目以 `=` 开头）。
pub const EV_ENAME: u32 = SEG_BASE + 0x02;
/// 空条目。
pub const EV_EENTRY: u32 = SEG_BASE + 0x03;
/// 重复键。
pub const EV_EDUP: u32 = SEG_BASE + 0x04;
/// 条目内嵌 NUL。
pub const EV_EEMBED: u32 = SEG_BASE + 0x05;
/// 条目数超过上限。
pub const EV_EFULL: u32 = SEG_BASE + 0x06;
/// 名字超长。
pub const EV_EKEY: u32 = SEG_BASE + 0x07;
/// 值超长。
pub const EV_EVAL: u32 = SEG_BASE + 0x08;
/// 键未找到。
pub const EV_ENOENT: u32 = SEG_BASE + 0x09;
/// 读取偏移越过末尾。
pub const EV_EOFF: u32 = SEG_BASE + 0x0a;
/// 条目下标越界。
pub const EV_EINDEX: u32 = SEG_BASE + 0x0b;
/// 字节下标越过末尾。
pub const EV_ENOBS: u32 = SEG_BASE + 0x0c;
/// 账本失衡。
pub const EV_EBAL: u32 = SEG_BASE + 0x0d;
/// 账本槽位乱序。
pub const EV_EORDER: u32 = SEG_BASE + 0x0e;
/// 观测值与复算不符。
pub const EV_EMISMATCH: u32 = SEG_BASE + 0x0f;
/// 冻结后重放不一致。
pub const EV_ESEAL: u32 = SEG_BASE + 0x10;

/// 条目数上限。
pub const MAX_ENV: usize = 6;
/// 名字字节上限。
pub const KEY_CAP: usize = 8;
/// 值字节上限。
pub const VAL_CAP: usize = 12;
/// 单条目容量（名字 + `=` + 值）。
pub const ENTRY_CAP: usize = KEY_CAP + 1 + VAL_CAP;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：构造。
pub const OP_SET: u8 = 1;
/// 操作码：总长。
pub const OP_TOTAL: u8 = 2;
/// 操作码：条目数。
pub const OP_COUNT: u8 = 3;
/// 操作码：查询值。
pub const OP_GET: u8 = 4;
/// 操作码：取字节。
pub const OP_BYTE: u8 = 5;
/// 操作码：条目长度。
pub const OP_LEN: u8 = 6;
/// 操作码：带偏移读取。
pub const OP_READ: u8 = 7;

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

/// 环境块。
#[derive(Clone, Copy, Debug)]
pub struct Environ {
    slots: [[u8; ENTRY_CAP]; MAX_ENV],
    lens: [usize; MAX_ENV],
    count: usize,
    total: usize,
}

/// 在条目里找第一个 `=` 的下标。
fn eq_pos(entry: &[u8]) -> Option<usize> {
    let mut i = 0usize;
    while i < entry.len() {
        if let Some(b) = entry.get(i) {
            if *b == b'=' {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 两段字节是否相等。
fn same_bytes(a: &[u8], b: &[u8]) -> bool {
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

impl Environ {
    /// 空环境块。
    pub const fn new() -> Environ {
        Environ {
            slots: [[0u8; ENTRY_CAP]; MAX_ENV],
            lens: [0usize; MAX_ENV],
            count: 0,
            total: 0,
        }
    }

    /// 构造环境块。
    ///
    /// 成功返回字节流总长（含每条的结尾 NUL）。校验全部通过才落盘。
    pub fn set(&mut self, entries: &[&[u8]]) -> Result<u32, u32> {
        if entries.len() > MAX_ENV {
            return Err(EV_EFULL);
        }
        let mut total = 0usize;
        let mut i = 0usize;
        while i < entries.len() {
            let a = match entries.get(i) {
                Some(v) => *v,
                None => return Err(EV_EFULL),
            };
            if a.is_empty() {
                return Err(EV_EENTRY);
            }
            let eq = match eq_pos(a) {
                Some(v) => v,
                None => return Err(EV_ENOEQ),
            };
            if eq == 0 {
                return Err(EV_ENAME);
            }
            let mut k = 0usize;
            while k < a.len() {
                if let Some(b) = a.get(k) {
                    if *b == 0 {
                        return Err(EV_EEMBED);
                    }
                }
                k += 1;
            }
            if eq > KEY_CAP {
                return Err(EV_EKEY);
            }
            if a.len() - eq - 1 > VAL_CAP {
                return Err(EV_EVAL);
            }
            // 与前面已校验过的条目比较键，拒绝重复。
            let mut j = 0usize;
            while j < i {
                if let Some(prev) = entries.get(j) {
                    if let (Some(peq), Some(key)) = (eq_pos(prev), a.get(..eq)) {
                        if let Some(pkey) = prev.get(..peq) {
                            if same_bytes(pkey, key) {
                                return Err(EV_EDUP);
                            }
                        }
                    }
                }
                j += 1;
            }
            total += a.len() + 1;
            i += 1;
        }

        // 全部合法，落盘。
        i = 0;
        while i < entries.len() {
            match entries.get(i) {
                Some(a) => {
                    let mut k = 0usize;
                    while k < a.len() {
                        if let (Some(src), Some(row)) = (a.get(k), self.slots.get_mut(i)) {
                            if let Some(dst) = row.get_mut(k) {
                                *dst = *src;
                            }
                        }
                        k += 1;
                    }
                    if let Some(l) = self.lens.get_mut(i) {
                        *l = a.len();
                    }
                }
                None => return Err(EV_EFULL),
            }
            i += 1;
        }
        self.count = entries.len();
        self.total = total;
        Ok(total as u32)
    }

    /// 条目数。
    pub fn count(&self) -> usize {
        self.count
    }

    /// 是否空环境。
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// 字节流总长（含结尾 NUL）。
    pub fn total(&self) -> usize {
        self.total
    }

    /// 第 i 条的原始字节（不含结尾 NUL）。
    pub fn entry(&self, i: usize) -> Result<&[u8], u32> {
        if i >= self.count {
            return Err(EV_EINDEX);
        }
        let l = match self.lens.get(i) {
            Some(v) => *v,
            None => return Err(EV_EINDEX),
        };
        match self.slots.get(i) {
            Some(row) => match row.get(..l) {
                Some(s) => Ok(s),
                None => Err(EV_EINDEX),
            },
            None => Err(EV_EINDEX),
        }
    }

    /// 第 i 条的等号位置。
    pub fn eq_at(&self, i: usize) -> Result<u32, u32> {
        let e = self.entry(i)?;
        match eq_pos(e) {
            Some(v) => Ok(v as u32),
            None => Err(EV_ENOEQ),
        }
    }

    /// 第 i 条的名字。
    pub fn name(&self, i: usize) -> Result<&[u8], u32> {
        let e = self.entry(i)?;
        match eq_pos(e) {
            Some(p) => match e.get(..p) {
                Some(s) => Ok(s),
                None => Err(EV_ENAME),
            },
            None => Err(EV_ENOEQ),
        }
    }

    /// 第 i 条的值（可以为空串）。
    pub fn value(&self, i: usize) -> Result<&[u8], u32> {
        let e = self.entry(i)?;
        match eq_pos(e) {
            Some(p) => match e.get(p + 1..) {
                Some(s) => Ok(s),
                None => Err(EV_EVAL),
            },
            None => Err(EV_ENOEQ),
        }
    }

    /// 按键查值。
    ///
    /// 名字必须整段相等；`HOME` 不会匹配 `HOMELESS`。未找到报 `ENOENT`。
    pub fn get(&self, key: &[u8]) -> Result<&[u8], u32> {
        let mut i = 0usize;
        while i < self.count {
            let e = match self.entry(i) {
                Ok(v) => v,
                Err(c) => return Err(c),
            };
            if let Some(p) = eq_pos(e) {
                if p == key.len() {
                    if let Some(nm) = e.get(..p) {
                        if same_bytes(nm, key) {
                            return match e.get(p + 1..) {
                                Some(v) => Ok(v),
                                None => Err(EV_EVAL),
                            };
                        }
                    }
                }
            }
            i += 1;
        }
        Err(EV_ENOENT)
    }

    /// 字节流中第 i 个字节（条目结尾 NUL 可取值 0）。
    pub fn byte_at(&self, i: usize) -> Result<u32, u32> {
        if i >= self.total {
            return Err(EV_ENOBS);
        }
        let mut off = 0usize;
        let mut a = 0usize;
        while a < self.count {
            let l = match self.lens.get(a) {
                Some(v) => *v,
                None => return Err(EV_ENOBS),
            };
            if i < off + l + 1 {
                let rel = i - off;
                if rel == l {
                    return Ok(0);
                }
                return match self.slots.get(a).and_then(|row| row.get(rel)) {
                    Some(b) => Ok(*b as u32),
                    None => Err(EV_ENOBS),
                };
            }
            off += l + 1;
            a += 1;
        }
        Err(EV_ENOBS)
    }

    /// 模拟带偏移的 `read(2)`。
    pub fn read_from(&self, off: usize, size: usize) -> Result<u32, u32> {
        if off > self.total {
            return Err(EV_EOFF);
        }
        let avail = self.total - off;
        let n = if size < avail { size } else { avail };
        Ok(n as u32)
    }

    /// 尾部 NUL 是否在位。
    pub fn ends_with_nul(&self) -> bool {
        if self.count == 0 {
            return true;
        }
        self.byte_at(self.total - 1) == Ok(0)
    }
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

    /// 登记一格；槽位必须严格递增。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(EV_EORDER);
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
            return Err(EV_EBAL);
        }
        const VALS: [u32; 8] = [32, 3, 5, 118, 32, 0, 10, 0];
        const CODES: [u32; 6] = [
            EV_ENOEQ,
            EV_ENAME,
            EV_EENTRY,
            EV_EDUP,
            EV_EEMBED,
            EV_EFULL,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(EV_EBAL),
            };
            if s.slot as usize != i {
                return Err(EV_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(EV_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(EV_EMISMATCH);
                        }
                    }
                    None => return Err(EV_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(EV_EMISMATCH);
                        }
                    }
                    None => return Err(EV_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(EV_EBAL);
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
            self.rejects += 1;
            return Err(EV_EBAL);
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
            Err(EV_ESEAL)
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

/// 十四格账本：三变量布局 → 查询/字节 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut e = Environ::new();

    let env: [&[u8]; 3] = [b"PATH=/bin", b"HOME=/root", b"TERM=vt100"];
    let _ = l.register(word_cell(0, OP_SET, e.set(&env)));
    let _ = l.register(cell(1, OP_COUNT, Ok(e.count())));
    let _ = l.register(cell(2, OP_GET, e.get(b"HOME").map(|v| v.len())));
    let _ = l.register(word_cell(3, OP_GET, e.get(b"TERM").map(|v| match v.first() {
        Some(b) => *b as u32,
        None => 0,
    })));
    let _ = l.register(cell(4, OP_TOTAL, Ok(e.total())));
    let _ = l.register(word_cell(5, OP_BYTE, e.byte_at(9)));
    let _ = l.register(cell(6, OP_LEN, e.entry(1).map(|x| x.len())));
    let _ = l.register(cell(7, OP_READ, e.read_from(32, 4).map(|n| n as usize)));

    let noeq: [&[u8]; 1] = [b"NOEQ"];
    let _ = l.register(word_cell(8, OP_SET, e.set(&noeq)));

    let noname: [&[u8]; 1] = [b"=x"];
    let _ = l.register(word_cell(9, OP_SET, e.set(&noname)));

    let blank: [&[u8]; 1] = [b""];
    let _ = l.register(word_cell(10, OP_SET, e.set(&blank)));

    let dup: [&[u8]; 2] = [b"A=1", b"A=2"];
    let _ = l.register(word_cell(11, OP_SET, e.set(&dup)));

    let embed: [&[u8]; 1] = [b"A\0B=1"];
    let _ = l.register(word_cell(12, OP_SET, e.set(&embed)));

    let many: [&[u8]; 7] = [b"A=1", b"B=1", b"C=1", b"D=1", b"E=1", b"F=1", b"G=1"];
    let _ = l.register(word_cell(13, OP_SET, e.set(&many)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：NUL 分隔布局与末尾 NUL。
    let env: [&[u8]; 3] = [b"PATH=/bin", b"HOME=/root", b"TERM=vt100"];
    let mut e = Environ::new();
    let set = e.set(&env);
    out[1] = set == Ok(32)
        && e.total() == 32
        && e.count() == 3
        && e.byte_at(9) == Ok(0)
        && e.byte_at(10) == Ok(b'H' as u32)
        && e.byte_at(20) == Ok(0)
        && e.byte_at(31) == Ok(0)
        && e.byte_at(32) == Err(EV_ENOBS)
        && e.ends_with_nul()
        && e.entry(0) == Ok(&b"PATH=/bin"[..])
        && e.name(1) == Ok(&b"HOME"[..])
        && e.value(2) == Ok(&b"vt100"[..])
        && e.eq_at(0) == Ok(4);

    // 二路：整名匹配与带偏移读取。
    let mut d = Environ::new();
    let _ = d.set(&env);
    out[2] = d.get(b"HOME") == Ok(&b"/root"[..])
        && d.get(b"PATH") == Ok(&b"/bin"[..])
        && d.get(b"TERM") == Ok(&b"vt100"[..])
        && d.get(b"NOPE") == Err(EV_ENOENT)
        && d.get(b"home") == Err(EV_ENOENT)
        && d.get(b"HOM") == Err(EV_ENOENT)
        && d.get(b"HOMELESS") == Err(EV_ENOENT)
        && d.read_from(0, 10) == Ok(10)
        && d.read_from(10, 11) == Ok(11)
        && d.read_from(21, 11) == Ok(11)
        && d.read_from(31, 1) == Ok(1)
        && d.read_from(32, 0) == Ok(0)
        && d.read_from(0, 100) == Ok(32)
        && d.read_from(33, 1) == Err(EV_EOFF);

    // 三路：闸门次序、长度上限、失败不改状态、空环境。
    let mut g = Environ::new();
    let _ = g.set(&env);
    let many: [&[u8]; 7] = [b"A=1", b"B=1", b"C=1", b"D=1", b"E=1", b"F=1", b"G=1"];
    let mixed: [&[u8]; 7] = [b"", b"NOEQ", b"=x", b"A\0B=1", b"A=1", b"B=2", b"C=3"];
    let count_first = g.set(&many) == Err(EV_EFULL) && g.set(&mixed) == Err(EV_EFULL);
    let blank: [&[u8]; 1] = [b""];
    let noeq: [&[u8]; 1] = [b"NOEQ"];
    let noname: [&[u8]; 1] = [b"=x"];
    let embed: [&[u8]; 1] = [b"A\0B=1"];
    let dup: [&[u8]; 2] = [b"A=1", b"A=2"];
    let longkey: [&[u8]; 1] = [b"ABCDEFGHI=1"];
    let longval: [&[u8]; 1] = [b"A=0123456789abc"];
    let exactval: [&[u8]; 1] = [b"A=0123456789ab"];
    let gates = g.set(&blank) == Err(EV_EENTRY)
        && g.set(&noeq) == Err(EV_ENOEQ)
        && g.set(&noname) == Err(EV_ENAME)
        && g.set(&embed) == Err(EV_EEMBED)
        && g.set(&dup) == Err(EV_EDUP)
        && g.set(&longkey) == Err(EV_EKEY)
        && g.set(&longval) == Err(EV_EVAL)
        && g.set(&exactval) == Ok(15)
        && g.count() == 1
        && g.get(b"A") == Ok(&b"0123456789ab"[..]);

    let z = Environ::new();
    let empty_ok = z.is_empty()
        && z.total() == 0
        && z.ends_with_nul()
        && z.read_from(0, 8) == Ok(0)
        && z.read_from(1, 1) == Err(EV_EOFF)
        && z.get(b"A") == Err(EV_ENOENT)
        && z.entry(0) == Err(EV_EINDEX);
    out[3] = count_first && gates && empty_ok;

    out
}

/// 十三项判据。
pub fn run_unx11345_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11345_environ_split");
    let l = build_ledger();

    s.add_dyn(
        "EV1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "EV2-总字节数含结尾NUL",
        l.sample(0).map(|x| x.value) == Some(32),
        "static",
    );

    s.add(
        "EV3-条目计数",
        l.sample(1).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "EV4-值查找长度",
        l.sample(2).map(|x| x.value) == Some(5),
        "static",
    );

    s.add(
        "EV5-值首字节",
        {
            let env: [&[u8]; 3] = [b"PATH=/bin", b"HOME=/root", b"TERM=vt100"];
            let mut e = Environ::new();
            let _ = e.set(&env);
            l.sample(3).map(|x| x.value) == Some(b'v' as u32)
                && e.value(2) == Ok(&b"vt100"[..])
                && e.value(1) == Ok(&b"/root"[..])
        },
        "static",
    );

    s.add(
        "EV6-NUL分隔位置",
        {
            let env: [&[u8]; 3] = [b"PATH=/bin", b"HOME=/root", b"TERM=vt100"];
            let mut e = Environ::new();
            let _ = e.set(&env);
            l.sample(4).map(|x| x.value) == Some(32)
                && l.sample(5).map(|x| x.value) == Some(0)
                && e.byte_at(9) == Ok(0)
                && e.byte_at(20) == Ok(0)
                && e.byte_at(31) == Ok(0)
        },
        "static",
    );

    s.add(
        "EV7-条目长度",
        l.sample(6).map(|x| x.value) == Some(10),
        "static",
    );

    s.add(
        "EV8-末尾EOF",
        l.sample(7).map(|x| x.value) == Some(0),
        "static",
    );

    s.add(
        "EV9-缺等号拒绝",
        l.sample(8).map(|x| x.code) == Some(EV_ENOEQ),
        "static",
    );

    s.add(
        "EV10-空名与空条目",
        l.sample(9).map(|x| x.code) == Some(EV_ENAME)
            && l.sample(10).map(|x| x.code) == Some(EV_EENTRY),
        "static",
    );

    s.add(
        "EV11-重复键拒绝",
        l.sample(11).map(|x| x.code) == Some(EV_EDUP),
        "static",
    );

    s.add(
        "EV12-内嵌NUL与容量",
        l.sample(12).map(|x| x.code) == Some(EV_EEMBED)
            && l.sample(13).map(|x| x.code) == Some(EV_EFULL),
        "static",
    );

    s.add(
        "EV13-冻结重放四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two.is_err()
                && f.is_frozen()
                && f.root() != 0
                && f.rejects() == 1
                && inj == [true; 4]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn three() -> Environ {
        let env: [&[u8]; 3] = [b"PATH=/bin", b"HOME=/root", b"TERM=vt100"];
        let mut e = Environ::new();
        let _ = e.set(&env);
        e
    }

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
        let s = run_unx11345_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn layout_and_nuls() {
        let e = three();
        assert_eq!(e.total(), 32);
        assert_eq!(e.count(), 3);
        assert_eq!(e.byte_at(9), Ok(0));
        assert_eq!(e.byte_at(20), Ok(0));
        assert_eq!(e.byte_at(31), Ok(0));
        assert_eq!(e.byte_at(32), Err(EV_ENOBS));
        assert!(e.ends_with_nul());
        assert_eq!(e.eq_at(1), Ok(4));
    }

    #[test]
    fn entries_name_value() {
        let e = three();
        assert_eq!(e.entry(0), Ok(&b"PATH=/bin"[..]));
        assert_eq!(e.name(0), Ok(&b"PATH"[..]));
        assert_eq!(e.value(0), Ok(&b"/bin"[..]));
        assert_eq!(e.get(b"HOME"), Ok(&b"/root"[..]));
        assert_eq!(e.get(b"HOM"), Err(EV_ENOENT));
        assert_eq!(e.get(b"HOME2"), Err(EV_ENOENT));
        assert_eq!(e.entry(3), Err(EV_EINDEX));
    }

    #[test]
    fn partial_reads() {
        let e = three();
        assert_eq!(e.read_from(0, 10), Ok(10));
        assert_eq!(e.read_from(10, 11), Ok(11));
        assert_eq!(e.read_from(31, 1), Ok(1));
        assert_eq!(e.read_from(32, 4), Ok(0));
        assert_eq!(e.read_from(33, 4), Err(EV_EOFF));
    }

    #[test]
    fn rejects_malformed_entries() {
        let mut e = Environ::new();
        let blank: [&[u8]; 1] = [b""];
        let noeq: [&[u8]; 1] = [b"NOEQ"];
        let noname: [&[u8]; 1] = [b"=x"];
        let embed: [&[u8]; 1] = [b"A\0B=1"];
        let dup: [&[u8]; 2] = [b"A=1", b"A=2"];
        assert_eq!(e.set(&blank), Err(EV_EENTRY));
        assert_eq!(e.set(&noeq), Err(EV_ENOEQ));
        assert_eq!(e.set(&noname), Err(EV_ENAME));
        assert_eq!(e.set(&embed), Err(EV_EEMBED));
        assert_eq!(e.set(&dup), Err(EV_EDUP));
        assert_eq!(e.count(), 0);
    }

    #[test]
    fn length_caps_exact() {
        let mut e = Environ::new();
        let longkey: [&[u8]; 1] = [b"ABCDEFGHI=1"];
        let longval: [&[u8]; 1] = [b"A=0123456789abc"];
        let exactkey: [&[u8]; 1] = [b"ABCDEFGH=1"];
        let exactval: [&[u8]; 1] = [b"A=0123456789ab"];
        assert_eq!(e.set(&longkey), Err(EV_EKEY));
        assert_eq!(e.set(&longval), Err(EV_EVAL));
        assert_eq!(e.set(&exactkey), Ok(11));
        assert_eq!(e.set(&exactval), Ok(15));
        assert_eq!(e.get(b"A"), Ok(&b"0123456789ab"[..]));
    }

    #[test]
    fn capacity_and_atomicity() {
        let mut e = three();
        let many: [&[u8]; 7] = [b"A=1", b"B=1", b"C=1", b"D=1", b"E=1", b"F=1", b"G=1"];
        assert_eq!(e.set(&many), Err(EV_EFULL));
        assert_eq!(e.count(), 3);
        assert_eq!(e.get(b"TERM"), Ok(&b"vt100"[..]));
    }

    #[test]
    fn empty_environment() {
        let mut e = Environ::new();
        assert_eq!(e.set(&[]), Ok(0));
        assert!(e.is_empty());
        assert_eq!(e.total(), 0);
        assert!(e.ends_with_nul());
        assert_eq!(e.read_from(0, 8), Ok(0));
        assert_eq!(e.read_from(1, 1), Err(EV_EOFF));
        assert_eq!(e.get(b"A"), Err(EV_ENOENT));
    }

    #[test]
    fn freeze_blocks_writes() {
        let mut l = build_ledger();
        assert!(l.freeze().is_ok());
        assert!(l.register(Sample::blank()).is_err());
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn inject_paths_agree() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
