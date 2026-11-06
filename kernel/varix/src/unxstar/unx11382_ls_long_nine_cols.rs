//! UNX-F11382 ls -l 长格式九列真值
//!
//! 长格式一行九列：`mode nlink owner group size mtime atime ctime name`。真值
//! 不在「拼出来像不像」，而在**同一张表里 name 列的起始偏移处处相同**：前八
//! 列按全表最宽值对齐，名字列从固定偏移开始，尺寸位数一变整列一起右移。
//!
//! 闸门次序：
//! - `open`：时钟为零（无法判定时间派生）；
//! - `push`：模式类型/高位非法 → 空名或超长 → 硬链接数为零 → 属主不可解析
//!   → 属组不可解析 → 表满 → 重名；
//! - `widths`：空表没有列宽真值；
//! - `line`：行下标越界。
//!
//! 账本形状：14 格 = 8 允 + 6 拒，6 拒覆盖 6 个不同域错误码。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xede0;

/// 模式位非法。
pub const LSL_EMODE: u32 = SEG_BASE + 0x01;
/// 名字为空或超长。
pub const LSL_ENAME: u32 = SEG_BASE + 0x02;
/// 硬链接数为零。
pub const LSL_ENLINK: u32 = SEG_BASE + 0x03;
/// 属主不可解析。
pub const LSL_ENOWN: u32 = SEG_BASE + 0x04;
/// 属组不可解析。
pub const LSL_ENGRP: u32 = SEG_BASE + 0x05;
/// 条目表满。
pub const LSL_EFULL: u32 = SEG_BASE + 0x06;
/// 名字重复。
pub const LSL_EDUP: u32 = SEG_BASE + 0x07;
/// 行下标越界。
pub const LSL_ERANGE: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const LSL_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const LSL_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const LSL_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const LSL_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const LSL_ENOBS: u32 = SEG_BASE + 0x0d;

/// 条目表容量。
pub const CAP: usize = 10;
/// 名字字节上限。
pub const NAME_MAX: usize = 16;
/// 权限位掩码。
pub const MODE_MASK: u32 = 0o7777;
/// 文件类型掩码。
pub const TYPE_MASK: u32 = 0o170000;
/// 九列。
pub const COLS: usize = 9;
/// 每个时间列固定 16 字节（`YYYY-MM-DD HH:MM`）。
pub const STAMP: u32 = 16;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：推入一行。
pub const OP_PUSH: u8 = 1;
/// 操作码：取表宽。
pub const OP_WIDTH: u8 = 2;
/// 操作码：取行宽。
pub const OP_LINE: u8 = 3;
/// 操作码：开户。
pub const OP_OPEN: u8 = 4;

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

/// 属主名表。
pub fn owner_name(uid: u32) -> Option<&'static [u8]> {
    match uid {
        0 => Some(b"root"),
        1 => Some(b"daemon"),
        1000 => Some(b"varix"),
        65534 => Some(b"nobody"),
        _ => None,
    }
}

/// 属组名表。
pub fn group_name(gid: u32) -> Option<&'static [u8]> {
    match gid {
        0 => Some(b"root"),
        1 => Some(b"daemon"),
        5 => Some(b"tty"),
        100 => Some(b"users"),
        _ => None,
    }
}

/// 十进制位数。
fn digits_u32(v: u32) -> u32 {
    let mut n = 1u32;
    let mut x = v;
    while x >= 10 {
        x /= 10;
        n += 1;
    }
    n
}

/// 十进制位数（64 位）。
fn digits_u64(v: u64) -> u32 {
    let mut n = 1u32;
    let mut x = v;
    while x >= 10 {
        x /= 10;
        n += 1;
    }
    n
}

/// 写一个模式串字节。
fn setb(out: &mut [u8; 10], i: usize, v: u8) {
    if let Some(d) = out.get_mut(i) {
        *d = v;
    }
}

/// 模式串（十位）。
pub fn mode_text(mode: u32) -> [u8; 10] {
    let mut out = [b'-'; 10];
    setb(
        &mut out,
        0,
        match mode & TYPE_MASK {
            0o040000 => b'd',
            0o120000 => b'l',
            0o100000 => b'-',
            _ => b'?',
        },
    );
    let mut i = 0usize;
    while i < 3 {
        let shift = 6u32.saturating_sub((i as u32).saturating_mul(3));
        let triad = (mode >> shift) & 0o7;
        setb(&mut out, i * 3 + 1, if triad & 0o4 != 0 { b'r' } else { b'-' });
        setb(&mut out, i * 3 + 2, if triad & 0o2 != 0 { b'w' } else { b'-' });
        setb(&mut out, i * 3 + 3, if triad & 0o1 != 0 { b'x' } else { b'-' });
        i += 1;
    }
    out
}

/// 一条长格式输入。
#[derive(Clone, Copy, Debug)]
pub struct Meta<'a> {
    /// 模式位。
    pub mode: u32,
    /// 硬链接数。
    pub nlink: u32,
    /// 属主 uid。
    pub uid: u32,
    /// 属组 gid。
    pub gid: u32,
    /// 字节数。
    pub size: u64,
    /// 修改时间。
    pub mtime: u64,
    /// 访问时间。
    pub atime: u64,
    /// 状态变更时间。
    pub ctime: u64,
    /// 名字。
    pub name: &'a [u8],
}

impl<'a> Meta<'a> {
    /// 常规默认：普通文件、单链接、root、零尺寸。
    pub fn new(name: &'a [u8]) -> Meta<'a> {
        Meta {
            mode: 0o100644,
            nlink: 1,
            uid: 0,
            gid: 0,
            size: 0,
            mtime: 0,
            atime: 0,
            ctime: 0,
            name,
        }
    }
}

/// 一行记录。
#[derive(Clone, Copy, Debug)]
struct Rec {
    mode: u32,
    nlink: u32,
    uid: u32,
    gid: u32,
    size: u64,
    mt: u64,
    at: u64,
    ct: u64,
    name: [u8; NAME_MAX],
    nlen: u8,
}

impl Rec {
    const fn blank() -> Rec {
        Rec {
            mode: 0o100644,
            nlink: 1,
            uid: 0,
            gid: 0,
            size: 0,
            mt: 0,
            at: 0,
            ct: 0,
            name: [0; NAME_MAX],
            nlen: 0,
        }
    }

    fn bytes(&self) -> &[u8] {
        match self.name.get(..self.nlen as usize) {
            Some(v) => v,
            None => &[],
        }
    }
}

/// 拷入名字。
fn put(dst: &mut [u8; NAME_MAX], src: &[u8]) -> u8 {
    let n = if src.len() > NAME_MAX { NAME_MAX } else { src.len() };
    let mut i = 0usize;
    while i < n {
        if let (Some(d), Some(s)) = (dst.get_mut(i), src.get(i)) {
            *d = *s;
        }
        i += 1;
    }
    n as u8
}

/// 字节序相等。
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

/// 长格式表。
#[derive(Clone, Copy, Debug)]
pub struct Long {
    now: u64,
    rows: [Rec; CAP],
    n: usize,
}

impl Long {
    /// 为时间派生开户。
    pub fn open(now: u64) -> Result<Long, u32> {
        if now == 0 {
            return Err(LSL_ERANGE);
        }
        Ok(Long { now, rows: [Rec::blank(); CAP], n: 0 })
    }

    /// 开户时钟。
    pub fn now(&self) -> u64 {
        self.now
    }

    /// 行数。
    pub fn count(&self) -> usize {
        self.n
    }

    /// 第 i 行。
    pub fn row_at(&self, i: usize) -> Option<(u32, u32, u32, u32, u64, u64, u64, u64, u8)> {
        match self.rows.get(i) {
            Some(v) if i < self.n => {
                Some((v.mode, v.nlink, v.uid, v.gid, v.size, v.mt, v.at, v.ct, v.nlen))
            }
            _ => None,
        }
    }

    /// 第 i 行的名字。
    pub fn name_at(&self, i: usize) -> Option<&[u8]> {
        match self.rows.get(i) {
            Some(v) if i < self.n => Some(v.bytes()),
            _ => None,
        }
    }

    /// 按名字找行。
    fn find(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            match self.rows.get(i) {
                Some(v) => {
                    if same_bytes(v.bytes(), name) {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 推入一行，返回累计行数。
    ///
    /// 闸门次序：模式 → 名字 → 硬链接 → 属主 → 属组 → 表满 → 重名。
    pub fn push(&mut self, m: Meta) -> Result<usize, u32> {
        let t = m.mode & TYPE_MASK;
        if t != 0o100000 && t != 0o040000 && t != 0o120000 {
            return Err(LSL_EMODE);
        }
        if m.mode & !(TYPE_MASK | MODE_MASK) != 0 {
            return Err(LSL_EMODE);
        }
        if m.name.is_empty() || m.name.len() > NAME_MAX {
            return Err(LSL_ENAME);
        }
        if m.nlink == 0 {
            return Err(LSL_ENLINK);
        }
        if owner_name(m.uid).is_none() {
            return Err(LSL_ENOWN);
        }
        if group_name(m.gid).is_none() {
            return Err(LSL_ENGRP);
        }
        if self.n >= CAP {
            return Err(LSL_EFULL);
        }
        if self.find(m.name).is_some() {
            return Err(LSL_EDUP);
        }
        let slot = match self.rows.get_mut(self.n) {
            Some(v) => v,
            None => return Err(LSL_EFULL),
        };
        slot.mode = m.mode;
        slot.nlink = m.nlink;
        slot.uid = m.uid;
        slot.gid = m.gid;
        slot.size = m.size;
        slot.mt = m.mtime;
        slot.at = m.atime;
        slot.ct = m.ctime;
        slot.nlen = put(&mut slot.name, m.name);
        self.n += 1;
        Ok(self.n)
    }

    /// 名字列起始偏移（九列对齐后的真值）。
    ///
    /// 闸门：空表没有列宽。
    pub fn widths(&self) -> Result<u32, u32> {
        if self.n == 0 {
            return Err(LSL_ERANGE);
        }
        let mut nw = 1u32;
        let mut ow = 0u32;
        let mut gw = 0u32;
        let mut sw = 1u32;
        let mut i = 0usize;
        while i < self.n {
            let v = match self.rows.get(i) {
                Some(r) => *r,
                None => return Err(LSL_ERANGE),
            };
            let d = digits_u32(v.nlink);
            if d > nw {
                nw = d;
            }
            let o = match owner_name(v.uid) {
                Some(s) => s.len() as u32,
                None => return Err(LSL_ENOWN),
            };
            if o > ow {
                ow = o;
            }
            let g = match group_name(v.gid) {
                Some(s) => s.len() as u32,
                None => return Err(LSL_ENGRP),
            };
            if g > gw {
                gw = g;
            }
            let s = digits_u64(v.size);
            if s > sw {
                sw = s;
            }
            i += 1;
        }
        let mut total = 10u32;
        total = total.saturating_add(nw);
        total = total.saturating_add(ow);
        total = total.saturating_add(gw);
        total = total.saturating_add(sw);
        total = total.saturating_add(STAMP.saturating_mul(3));
        total = total.saturating_add((COLS as u32).saturating_sub(1));
        Ok(total)
    }

    /// 第 i 行的整行字节数。
    ///
    /// 闸门：行下标越界。
    pub fn line(&self, idx: usize) -> Result<u32, u32> {
        if idx >= self.n {
            return Err(LSL_ERANGE);
        }
        let off = self.widths()?;
        let v = match self.rows.get(idx) {
            Some(r) => *r,
            None => return Err(LSL_ERANGE),
        };
        Ok(off.saturating_add(v.nlen as u32))
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold(h, self.now);
        let mut i = 0usize;
        while i < self.n {
            if let Some(v) = self.rows.get(i) {
                h = fold(h, v.mode as u64);
                h = fold(h, v.nlink as u64);
                h = fold(h, v.uid as u64);
                h = fold(h, v.gid as u64);
                h = fold(h, v.size);
                h = fold(h, v.mt);
                h = fold(h, v.at);
                h = fold(h, v.ct);
                let mut k = 0usize;
                let b = v.bytes();
                while k < b.len() {
                    if let Some(x) = b.get(k) {
                        h = fold(h, *x as u64);
                    }
                    k += 1;
                }
                h = fold(h, 0xff);
            }
            i += 1;
        }
        fold(h, self.n as u64)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.now == 0 || self.n > CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            let v = match self.rows.get(i) {
                Some(r) => *r,
                None => return false,
            };
            let t = v.mode & TYPE_MASK;
            if t != 0o100000 && t != 0o040000 && t != 0o120000 {
                return false;
            }
            if v.mode & !(TYPE_MASK | MODE_MASK) != 0 {
                return false;
            }
            if v.nlen == 0 || v.nlen as usize > NAME_MAX || v.nlink == 0 {
                return false;
            }
            if owner_name(v.uid).is_none() || group_name(v.gid).is_none() {
                return false;
            }
            if v.mt > self.now || v.at > self.now || v.ct > self.now {
                return false;
            }
            let mut j = 0usize;
            while j < i {
                match self.rows.get(j) {
                    Some(p) => {
                        if same_bytes(p.bytes(), v.bytes()) {
                            return false;
                        }
                    }
                    None => return false,
                }
                j += 1;
            }
            i += 1;
        }
        true
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

    /// 登记一格。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(LSL_ESLOT);
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
            return Err(LSL_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 79, 80, 4, 85, 81];
        const CODES: [u32; 6] =
            [LSL_EMODE, LSL_ENAME, LSL_ENLINK, LSL_ENOWN, LSL_ENGRP, LSL_EFULL];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(LSL_EBAL),
            };
            if s.slot as usize != i {
                return Err(LSL_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(LSL_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(LSL_EVALUE);
                        }
                    }
                    None => return Err(LSL_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(LSL_EVALUE);
                        }
                    }
                    None => return Err(LSL_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(LSL_EBAL);
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
            return Err(LSL_EBAL);
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
            Err(LSL_ESEAL)
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

/// 十四格账本：8 允 + 6 拒。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut a = match Long::open(1_700_000_000) {
        Ok(v) => v,
        Err(_) => return l,
    };

    let _ = l.register(cell(
        0,
        OP_PUSH,
        a.push(Meta { size: 7, mtime: 1000, atime: 1000, ctime: 1000, ..Meta::new(b"a") }),
    ));
    let _ = l.register(cell(
        1,
        OP_PUSH,
        a.push(Meta {
            mode: 0o100600,
            size: 0,
            mtime: 1000,
            atime: 1000,
            ctime: 1000,
            ..Meta::new(b"bb")
        }),
    ));
    let _ = l.register(cell(
        2,
        OP_PUSH,
        a.push(Meta {
            mode: 0o040755,
            size: 4096,
            mtime: 1000,
            atime: 1000,
            ctime: 1000,
            ..Meta::new(b"ccc")
        }),
    ));
    let _ = l.register(word_cell(3, OP_WIDTH, a.widths()));
    let _ = l.register(word_cell(4, OP_LINE, a.line(0)));

    let _ = l.register(cell(
        5,
        OP_PUSH,
        a.push(Meta {
            size: 123456,
            mtime: 1500,
            atime: 1500,
            ctime: 1500,
            ..Meta::new(b"dddd")
        }),
    ));
    let _ = l.register(word_cell(6, OP_LINE, a.line(3)));
    let _ = l.register(word_cell(7, OP_WIDTH, a.widths()));

    let _ = l.register(cell(8, OP_PUSH, a.push(Meta { mode: 0o200644, ..Meta::new(b"e") })));
    let _ = l.register(cell(9, OP_PUSH, a.push(Meta::new(b""))));
    let _ = l.register(cell(10, OP_PUSH, a.push(Meta { nlink: 0, ..Meta::new(b"e") })));
    let _ = l.register(cell(11, OP_PUSH, a.push(Meta { uid: 4242, ..Meta::new(b"e") })));
    let _ = l.register(cell(12, OP_PUSH, a.push(Meta { gid: 999, ..Meta::new(b"e") })));

    let mut i = 0u32;
    while i < 6 {
        let name: [u8; 2] = [b'f', b'0' + i as u8];
        let _ = a.push(Meta::new(&name));
        i += 1;
    }
    let _ = l.register(cell(13, OP_PUSH, a.push(Meta::new(b"g"))));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：三行九列、名字列偏移恒定。
    let mut a = match Long::open(1_700_000_000) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let r1 = a.push(Meta { size: 7, ..Meta::new(b"a") });
    let r2 = a.push(Meta { mode: 0o040755, size: 4096, ..Meta::new(b"ccc") });
    let w = a.widths();
    let l0 = a.line(0);
    let l1 = a.line(1);
    out[1] = r1 == Ok(1)
        && r2 == Ok(2)
        && w == Ok(79)
        && l0 == Ok(80)
        && l1 == Ok(82)
        && a.name_at(1) == Some(&b"ccc"[..])
        && a.count() == 2
        && a.consistent();

    // 二路：八类失败各自可辨，失败不改状态。
    let mut b = match Long::open(1_700_000_000) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let bad_mode = b.push(Meta { mode: 0o200644, ..Meta::new(b"x") }) == Err(LSL_EMODE);
    let long = b.push(Meta::new(b"0123456789abcdefg")) == Err(LSL_ENAME);
    let empty = b.push(Meta::new(b"")) == Err(LSL_ENAME);
    let link = b.push(Meta { nlink: 0, ..Meta::new(b"x") }) == Err(LSL_ENLINK);
    let own = b.push(Meta { uid: 7, ..Meta::new(b"x") }) == Err(LSL_ENOWN);
    let grp = b.push(Meta { gid: 7, ..Meta::new(b"x") }) == Err(LSL_ENGRP);
    let untouched = b.count() == 0 && b.widths() == Err(LSL_ERANGE);
    let first = b.push(Meta::new(b"a")) == Ok(1);
    let dup = b.push(Meta::new(b"a")) == Err(LSL_EDUP);
    let range = b.line(1) == Err(LSL_ERANGE);
    let clock = Long::open(0).is_err();
    out[2] = bad_mode
        && long
        && empty
        && link
        && own
        && grp
        && untouched
        && first
        && dup
        && range
        && clock
        && b.consistent();

    // 三路：表满与宽度随尺寸位数伸缩。
    let mut q = match Long::open(1_700_000_000) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let mut i = 0u32;
    while (i as usize) < CAP {
        let name: [u8; 2] = [b'n', b'0' + i as u8];
        let _ = q.push(Meta::new(&name));
        i += 1;
    }
    let q1 = q.count() == CAP;
    let q2 = q.push(Meta::new(b"zz")) == Err(LSL_EFULL);
    let mut w1 = match Long::open(1_700_000_000) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let _ = w1.push(Meta { size: 9, ..Meta::new(b"a") });
    let mut w2 = match Long::open(1_700_000_000) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let _ = w2.push(Meta { size: 123456, ..Meta::new(b"a") });
    let q3 = w1.widths() == Ok(76);
    let q4 = w2.widths() == Ok(81);
    out[3] = q1 && q2 && q3 && q4 && q.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11382_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11382.ls_long_nine_cols");
    let l = build_ledger();

    s.add_dyn(
        "LSL1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "LSL2-九列名偏移恒定",
        l.sample(3).map(|x| x.value) == Some(79)
            && l.sample(4).map(|x| x.value) == Some(80)
            && l.sample(6).map(|x| x.value) == Some(85)
            && l.sample(7).map(|x| x.value) == Some(81),
        "static",
    );

    s.add(
        "LSL3-推入累计",
        l.sample(0).map(|x| x.value) == Some(1)
            && l.sample(1).map(|x| x.value) == Some(2)
            && l.sample(2).map(|x| x.value) == Some(3)
            && l.sample(5).map(|x| x.value) == Some(4),
        "static",
    );

    s.add(
        "LSL4-非法模式被拒",
        {
            let mut a = match Long::open(1_700_000_000) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let bad_type = a.push(Meta { mode: 0o200644, ..Meta::new(b"x") }) == Err(LSL_EMODE);
            let bad_high = a.push(Meta { mode: 0o100644 | 0o10000, ..Meta::new(b"x") })
                == Err(LSL_EMODE);
            bad_type
                && bad_high
                && a.count() == 0
                && a.consistent()
                && l.sample(8).map(|x| x.code) == Some(LSL_EMODE)
        },
        "static",
    );

    s.add(
        "LSL5-空名被拒",
        {
            let mut a = match Long::open(1_700_000_000) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.push(Meta::new(b"")) == Err(LSL_ENAME)
                && a.push(Meta::new(b"0123456789abcdefg")) == Err(LSL_ENAME)
                && a.count() == 0
                && l.sample(9).map(|x| x.code) == Some(LSL_ENAME)
        },
        "static",
    );

    s.add(
        "LSL6-硬链接数为零被拒",
        {
            let mut a = match Long::open(1_700_000_000) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.push(Meta { nlink: 0, ..Meta::new(b"x") }) == Err(LSL_ENLINK)
                && a.count() == 0
                && l.sample(10).map(|x| x.code) == Some(LSL_ENLINK)
        },
        "static",
    );

    s.add(
        "LSL7-未知属主被拒",
        {
            let mut a = match Long::open(1_700_000_000) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.push(Meta { uid: 4242, ..Meta::new(b"x") }) == Err(LSL_ENOWN)
                && a.count() == 0
                && l.sample(11).map(|x| x.code) == Some(LSL_ENOWN)
        },
        "static",
    );

    s.add(
        "LSL8-未知属组被拒",
        {
            let mut a = match Long::open(1_700_000_000) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.push(Meta { gid: 999, ..Meta::new(b"x") }) == Err(LSL_ENGRP)
                && a.count() == 0
                && l.sample(12).map(|x| x.code) == Some(LSL_ENGRP)
        },
        "static",
    );

    s.add(
        "LSL9-表满被拒",
        {
            let mut a = match Long::open(1_700_000_000) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut i = 0u32;
            while (i as usize) < CAP {
                let name: [u8; 2] = [b'n', b'0' + i as u8];
                let _ = a.push(Meta::new(&name));
                i += 1;
            }
            a.count() == CAP
                && a.push(Meta::new(b"zz")) == Err(LSL_EFULL)
                && a.push(Meta::new(b"n0")) == Err(LSL_EFULL)
                && a.consistent()
                && l.sample(13).map(|x| x.code) == Some(LSL_EFULL)
        },
        "static",
    );

    s.add(
        "LSL10-行下标越界",
        {
            let empty = match Long::open(1_700_000_000) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut a = match Long::open(1_700_000_000) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.push(Meta::new(b"a"));
            empty.widths() == Err(LSL_ERANGE)
                && empty.line(0) == Err(LSL_ERANGE)
                && a.line(1) == Err(LSL_ERANGE)
                && Long::open(0).is_err()
                && empty.consistent()
        },
        "static",
    );

    s.add(
        "LSL11-宽度随尺寸位数",
        {
            let mut a = match Long::open(1_700_000_000) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut b = match Long::open(1_700_000_000) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.push(Meta { size: 9, ..Meta::new(b"a") });
            let _ = b.push(Meta { size: 123456, ..Meta::new(b"a") });
            a.widths() == Ok(76)
                && b.widths() == Ok(81)
                && b.widths().ok().zip(a.widths().ok()) == Some((81, 76))
                && a.consistent()
                && b.consistent()
        },
        "static",
    );

    s.add(
        "LSL12-模式串十位真值",
        &mode_text(0o100644) == b"-rw-r--r--"
            && &mode_text(0o040755) == b"drwxr-xr-x"
            && &mode_text(0o120777) == b"lrwxrwxrwx"
            && &mode_text(0o100600) == b"-rw-------"
            && &mode_text(0o040000) == b"d---------"
            && &mode_text(0o200000) == b"?---------",
        "static",
    );

    s.add(
        "LSL13-冻结重放四注入",
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

    fn long() -> Long {
        match Long::open(1_700_000_000) {
            Ok(v) => v,
            Err(_) => panic!("open"),
        }
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.rejects(), 0);
        assert!(l.audit().is_ok());
        let mut i = 0usize;
        while i < l.len() {
            assert_eq!(l.sample(i).map(|x| x.slot as usize), Some(i));
            i += 1;
        }
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11382_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn nine_columns_constant_offset() {
        let mut a = long();
        assert_eq!(a.push(Meta::new(b"a")), Ok(1));
        assert_eq!(a.push(Meta { size: 4096, ..Meta::new(b"bb") }), Ok(2));
        let off = a.widths();
        assert_eq!(off, Ok(79));
        assert_eq!(a.line(0), Ok(80));
        assert_eq!(a.line(1), Ok(81));
        assert!(a.consistent());
    }

    #[test]
    fn mode_gate() {
        let mut a = long();
        assert_eq!(a.push(Meta { mode: 0o200644, ..Meta::new(b"x") }), Err(LSL_EMODE));
        assert_eq!(a.push(Meta { mode: 0o100644 | 0o10000, ..Meta::new(b"x") }), Err(LSL_EMODE));
        assert_eq!(a.push(Meta::new(b"x")), Ok(1));
    }

    #[test]
    fn owner_group_gate() {
        let mut a = long();
        assert_eq!(a.push(Meta { uid: 4242, ..Meta::new(b"x") }), Err(LSL_ENOWN));
        assert_eq!(a.push(Meta { gid: 999, ..Meta::new(b"x") }), Err(LSL_ENGRP));
        assert_eq!(a.push(Meta { uid: 1000, gid: 100, ..Meta::new(b"x") }), Ok(1));
        assert_eq!(a.widths(), Ok(78));
    }

    #[test]
    fn duplicate_gate() {
        let mut a = long();
        assert_eq!(a.push(Meta::new(b"a")), Ok(1));
        assert_eq!(a.push(Meta::new(b"a")), Err(LSL_EDUP));
        assert_eq!(a.count(), 1);
    }

    #[test]
    fn capacity_limit() {
        let mut a = long();
        let mut i = 0usize;
        while i < CAP {
            let name: [u8; 2] = [b'n', b'0' + i as u8];
            assert_eq!(a.push(Meta::new(&name)), Ok(i + 1));
            i += 1;
        }
        assert_eq!(a.push(Meta::new(b"zz")), Err(LSL_EFULL));
        assert!(a.consistent());
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
