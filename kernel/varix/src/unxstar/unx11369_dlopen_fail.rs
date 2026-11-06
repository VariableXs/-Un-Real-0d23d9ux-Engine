//! UNX-F11369 dlopen 失败返回语义：NULL 与 dlerror 的配对（0xed10 段）
//!
//! `dlopen` 失败时返回 NULL，**同时**把一条报文放进本线程的错误槽；成功时返回
//! 非 NULL，并清掉之前残留的报文。这两件事必须成对出现，否则调用方会读到
//! 上一次失败的报文，把「这次成功」误判成「这次失败」。
//!
//! glibc 的装载失败至少分成五类，报文各不相同：
//!
//! 1. 文件不存在（`cannot open shared object file: No such file or directory`）；
//! 2. ELF 架构不符（`wrong ELF class` / `cannot open shared object file` 之前的
//!    格式判定）；
//! 3. 立即绑定下存在未定义符号（`undefined symbol: ...`）；
//! 4. 依赖成环（`cannot load ... : ...`，链接器检测到 DT_NEEDED 闭环）；
//! 5. 构造函数失败（`... : ...`，init 返回非零）。
//!
//! 本模块把这五类分别编码为 `ENOTFOUND / EBADARCH / EUNSYM / ECYCLE /
//! EINITFAIL`，并为每类维护一条独立的报文编号，判据要求五个编号互不相同。
//!
//! 一个容易忽略的细节：**参数错误不发布报文**。`dlopen(NULL)` 是合法的（返回
//! 主程序句柄），而这里把空键视为调用约定错误，返回 `EINVAL` 且**不**触碰错误
//! 槽——因为「你调用错了」不是「装载失败」，混进来会污染配对计数。
//!
//! 闸门次序：空键 → 查表 → 架构 → 依赖环 → 未定义符号 → 构造函数 → 成功发布。
//! 之所以把架构判定放在依赖环之前：一个架构都不对的映像，讨论它的依赖没有意义，
//! 报文必须指向最外层、最容易被用户修正的那个错误。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xed10;

/// 键为零（调用约定错误，不发布报文）。
pub const DF_EINVAL: u32 = SEG_BASE + 0x01;
/// 找不到映像。
pub const DF_ENOTFOUND: u32 = SEG_BASE + 0x02;
/// ELF 架构不符。
pub const DF_EBADARCH: u32 = SEG_BASE + 0x03;
/// 立即绑定下存在未定义符号。
pub const DF_EUNSYM: u32 = SEG_BASE + 0x04;
/// 依赖成环。
pub const DF_ECYCLE: u32 = SEG_BASE + 0x05;
/// 构造函数失败。
pub const DF_EINITFAIL: u32 = SEG_BASE + 0x06;
/// 没有待取报文。
pub const DF_ENOMSG: u32 = SEG_BASE + 0x07;
/// 映像表已满。
pub const DF_EFULL: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const DF_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const DF_EORDER: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const DF_EMISMATCH: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const DF_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const DF_ENOBS: u32 = SEG_BASE + 0x0d;

/// 映像表容量。
pub const CAP: usize = 8;
/// 报文编号基数。
pub const MSG_BASE: u32 = 0x1_0000;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：登记映像。
pub const OP_ADD: u8 = 1;
/// 操作码：打开。
pub const OP_OPEN: u8 = 2;
/// 操作码：关闭。
pub const OP_CLOSE: u8 = 3;
/// 操作码：取报文。
pub const OP_TAKE: u8 = 4;

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

/// 映像描述。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Image {
    /// 映像键。
    pub key: u32,
    /// 架构编号（1 为宿主架构）。
    pub arch: u8,
    /// 依赖的映像键（0 表示无依赖）。
    pub needs: u32,
    /// 立即绑定下是否有未定义符号。
    pub unsym: bool,
    /// 构造函数是否失败。
    pub init_fail: bool,
    /// 装载计数。
    pub refs: u16,
}

impl Image {
    /// 造映像。
    pub const fn make(key: u32, arch: u8, needs: u32, unsym: bool, init_fail: bool) -> Image {
        Image { key, arch, needs, unsym, init_fail, refs: 0 }
    }

    /// 是否可装载。
    pub fn loadable(&self) -> bool {
        self.arch == 1 && !self.unsym && !self.init_fail
    }
}

/// 装载失败类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailKind {
    /// 找不到。
    Missing,
    /// 架构不符。
    Arch,
    /// 未定义符号。
    Unsym,
    /// 依赖成环。
    Cycle,
    /// 构造函数失败。
    Init,
}

impl FailKind {
    /// 类别编号（1..=5）。
    pub fn code(self) -> u32 {
        match self {
            FailKind::Missing => 1,
            FailKind::Arch => 2,
            FailKind::Unsym => 3,
            FailKind::Cycle => 4,
            FailKind::Init => 5,
        }
    }

    /// 报文编号。
    pub fn text(self) -> u32 {
        MSG_BASE + self.code()
    }

    /// 由编号还原类别。
    pub fn of(code: u32) -> Option<FailKind> {
        match code {
            1 => Some(FailKind::Missing),
            2 => Some(FailKind::Arch),
            3 => Some(FailKind::Unsym),
            4 => Some(FailKind::Cycle),
            5 => Some(FailKind::Init),
            _ => None,
        }
    }
}

/// NULL 与 dlerror 的配对器。
#[derive(Clone, Copy, Debug)]
pub struct Opener {
    imgs: [Image; CAP],
    n: usize,
    pending: u8,
    nulls: u32,
    reports: u32,
    clears: u32,
    takes: u32,
}

impl Opener {
    /// 空表。
    pub const fn new() -> Opener {
        Opener { imgs: [Image::make(0, 0, 0, false, false); CAP], n: 0, pending: 0, nulls: 0, reports: 0, clears: 0, takes: 0 }
    }

    /// 登记映像（重复即幂等）。
    pub fn add(&mut self, img: Image) -> Result<usize, u32> {
        if img.key == 0 {
            return Err(DF_EINVAL);
        }
        let mut i = 0usize;
        while i < self.n {
            if let Some(v) = self.imgs.get(i) {
                if v.key == img.key {
                    return Ok(i + 1);
                }
            }
            i += 1;
        }
        if self.n >= CAP {
            return Err(DF_EFULL);
        }
        if let Some(dst) = self.imgs.get_mut(self.n) {
            *dst = img;
        }
        self.n += 1;
        Ok(self.n)
    }

    /// 按键定位。
    fn idx_of(&self, key: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            if let Some(v) = self.imgs.get(i) {
                if v.key == key {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 依赖链是否成环。
    fn has_cycle(&self, start: usize) -> bool {
        let mut cur = start;
        let mut steps = 0usize;
        while steps <= self.n {
            let v = match self.imgs.get(cur) {
                Some(x) => *x,
                None => return false,
            };
            if v.needs == 0 {
                return false;
            }
            if v.needs == match self.imgs.get(start) {
                Some(s) => s.key,
                None => 0,
            } {
                return true;
            }
            cur = match self.idx_of(v.needs) {
                Some(k) => k,
                None => return false,
            };
            steps += 1;
        }
        true
    }

    /// 发布报文（内部）。
    fn publish(&mut self, kind: FailKind) {
        self.pending = kind.code() as u8;
        self.reports = self.reports.saturating_add(1);
    }

    /// 打开映像。
    pub fn open(&mut self, key: u32) -> Result<u32, u32> {
        if key == 0 {
            return Err(DF_EINVAL);
        }
        let i = match self.idx_of(key) {
            Some(v) => v,
            None => {
                self.nulls = self.nulls.saturating_add(1);
                self.publish(FailKind::Missing);
                return Err(DF_ENOTFOUND);
            }
        };
        let v = match self.imgs.get(i) {
            Some(x) => *x,
            None => return Err(DF_ENOTFOUND),
        };
        if v.arch != 1 {
            self.nulls = self.nulls.saturating_add(1);
            self.publish(FailKind::Arch);
            return Err(DF_EBADARCH);
        }
        if self.has_cycle(i) {
            self.nulls = self.nulls.saturating_add(1);
            self.publish(FailKind::Cycle);
            return Err(DF_ECYCLE);
        }
        if v.unsym {
            self.nulls = self.nulls.saturating_add(1);
            self.publish(FailKind::Unsym);
            return Err(DF_EUNSYM);
        }
        if v.init_fail {
            self.nulls = self.nulls.saturating_add(1);
            self.publish(FailKind::Init);
            return Err(DF_EINITFAIL);
        }
        if self.pending != 0 {
            self.clears = self.clears.saturating_add(1);
            self.pending = 0;
        }
        if let Some(dst) = self.imgs.get_mut(i) {
            dst.refs = dst.refs.saturating_add(1);
        }
        Ok((i as u32) + 1)
    }

    /// 关闭（计数减一）。
    pub fn close(&mut self, handle: u32) -> Result<usize, u32> {
        if handle == 0 || (handle as usize) > self.n {
            return Err(DF_EINVAL);
        }
        let i = (handle as usize) - 1;
        let v = match self.imgs.get(i) {
            Some(x) => *x,
            None => return Err(DF_EINVAL),
        };
        if v.refs == 0 {
            return Err(DF_EINVAL);
        }
        if let Some(dst) = self.imgs.get_mut(i) {
            dst.refs = dst.refs.saturating_sub(1);
        }
        Ok(v.refs.saturating_sub(1) as usize)
    }

    /// 取报文（一次一条，取后即清）。
    pub fn take_error(&mut self) -> Result<u32, u32> {
        if self.pending == 0 {
            return Err(DF_ENOMSG);
        }
        let kind = self.pending;
        self.pending = 0;
        self.takes = self.takes.saturating_add(1);
        match FailKind::of(kind as u32) {
            Some(k) => Ok(k.text()),
            None => Err(DF_ENOMSG),
        }
    }

    /// 是否有待取报文。
    pub fn has_error(&self) -> bool {
        self.pending != 0
    }

    /// NULL 返回次数。
    pub fn nulls(&self) -> u32 {
        self.nulls
    }

    /// 报文发布次数。
    pub fn reports(&self) -> u32 {
        self.reports
    }

    /// 成功清除残留报文的次数。
    pub fn clears(&self) -> u32 {
        self.clears
    }

    /// 取报文次数。
    pub fn takes(&self) -> u32 {
        self.takes
    }

    /// 配对成立：每次 NULL 都有报文，每次报文来自一次 NULL。
    pub fn paired(&self) -> bool {
        self.nulls == self.reports
    }

    /// 引用计数。
    pub fn refs_of(&self, key: u32) -> Result<usize, u32> {
        match self.idx_of(key) {
            Some(i) => match self.imgs.get(i) {
                Some(v) => Ok(v.refs as usize),
                None => Err(DF_ENOTFOUND),
            },
            None => Err(DF_ENOTFOUND),
        }
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
            return Err(DF_EORDER);
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
            return Err(DF_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 1, 1, 1, 1, 4];
        const CODES: [u32; 6] =
            [DF_EBADARCH, DF_EUNSYM, DF_ENOTFOUND, DF_EINVAL, DF_ECYCLE, DF_EINITFAIL];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(DF_EBAL),
            };
            if s.slot as usize != i {
                return Err(DF_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(DF_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(DF_EMISMATCH);
                        }
                    }
                    None => return Err(DF_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(DF_EMISMATCH);
                        }
                    }
                    None => return Err(DF_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(DF_EBAL);
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
            return Err(DF_EBAL);
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
            Err(DF_ESEAL)
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

/// 十四格账本：三登记四装载，架构、符号、缺件、空键、环、构造六拒。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut o = Opener::new();

    let _ = l.register(cell(0, OP_ADD, o.add(Image::make(0x11, 1, 0, false, false))));
    let _ = l.register(cell(1, OP_ADD, o.add(Image::make(0x12, 2, 0, false, false))));
    let _ = l.register(cell(2, OP_ADD, o.add(Image::make(0x13, 1, 0, true, false))));
    let _ = l.register(word_cell(3, OP_OPEN, o.open(0x11)));
    let _ = l.register(word_cell(4, OP_OPEN, o.open(0x11)));
    let _ = l.register(cell(5, OP_CLOSE, o.close(1)));
    let _ = l.register(word_cell(6, OP_OPEN, o.open(0x11)));
    let _ = l.register(cell(7, OP_ADD, o.add(Image::make(0x14, 1, 0x15, false, false))));

    let _ = l.register(word_cell(8, OP_OPEN, o.open(0x12)));

    let _ = l.register(word_cell(9, OP_OPEN, o.open(0x13)));

    let _ = l.register(word_cell(10, OP_OPEN, o.open(0x99)));

    let _ = l.register(word_cell(11, OP_OPEN, o.open(0)));

    let _ = o.add(Image::make(0x15, 1, 0x14, false, false));
    let _ = l.register(word_cell(12, OP_OPEN, o.open(0x14)));

    let _ = o.add(Image::make(0x16, 1, 0, false, true));
    let _ = l.register(word_cell(13, OP_OPEN, o.open(0x16)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：成功返回非 NULL、清掉残留，且无报文。
    let mut a = Opener::new();
    let _ = a.add(Image::make(0x21, 1, 0, false, false));
    let _ = a.add(Image::make(0x22, 1, 0, true, false));
    let bad = a.open(0x22);
    let had = a.has_error();
    let h = a.open(0x21);
    let after = a.has_error();
    let none = a.take_error();
    let refs = a.refs_of(0x21);
    out[1] = bad == Err(DF_EUNSYM)
        && had
        && h == Ok(1)
        && !after
        && none == Err(DF_ENOMSG)
        && refs == Ok(1)
        && a.nulls() == 1
        && a.reports() == 1
        && a.clears() == 1
        && a.paired();

    // 二路：五类失败、五条报文互不相同、逐条一次消费。
    let mut b = Opener::new();
    let _ = b.add(Image::make(0x31, 2, 0, false, false));
    let _ = b.add(Image::make(0x32, 1, 0, true, false));
    let _ = b.add(Image::make(0x33, 1, 0x34, false, false));
    let _ = b.add(Image::make(0x34, 1, 0x33, false, false));
    let _ = b.add(Image::make(0x35, 1, 0, false, true));
    let e1 = b.open(0x31);
    let m1 = b.take_error();
    let e2 = b.open(0x32);
    let m2 = b.take_error();
    let e3 = b.open(0x33);
    let m3 = b.take_error();
    let e4 = b.open(0x35);
    let m4 = b.take_error();
    let e5 = b.open(0x39);
    let m5 = b.take_error();
    let again = b.take_error();
    let distinct = m1 != m2 && m2 != m3 && m3 != m4 && m4 != m5 && m1 != m5;
    out[2] = e1 == Err(DF_EBADARCH)
        && e2 == Err(DF_EUNSYM)
        && e3 == Err(DF_ECYCLE)
        && e4 == Err(DF_EINITFAIL)
        && e5 == Err(DF_ENOTFOUND)
        && m1 == Ok(FailKind::Arch.text())
        && m2 == Ok(FailKind::Unsym.text())
        && m3 == Ok(FailKind::Cycle.text())
        && m4 == Ok(FailKind::Init.text())
        && m5 == Ok(FailKind::Missing.text())
        && again == Err(DF_ENOMSG)
        && distinct
        && b.nulls() == 5
        && b.reports() == 5
        && b.takes() == 5
        && b.paired();

    // 三路：闸门与容量。
    let mut c = Opener::new();
    let g1 = c.open(0);
    let no_msg = c.has_error();
    let g2 = c.open(0x99);
    let msg = c.has_error();
    let g3 = c.take_error();
    let g4 = c.take_error();
    let mut d = Opener::new();
    let mut i = 0u32;
    while (i as usize) < CAP {
        let _ = d.add(Image::make(0x40 + i, 1, 0, false, false));
        i += 1;
    }
    let full = d.add(Image::make(0x50, 1, 0, false, false));
    let dup = d.add(Image::make(0x40, 1, 0, false, false));
    let mut e = Opener::new();
    let _ = e.add(Image::make(0x60, 3, 0, false, false));
    let _ = e.open(0x60);
    let _ = e.add(Image::make(0x61, 1, 0, false, false));
    let cleared = e.open(0x61);
    let paired = e.nulls() == e.reports() && e.reports() == 1;
    out[3] = g1 == Err(DF_EINVAL)
        && !no_msg
        && g2 == Err(DF_ENOTFOUND)
        && msg
        && g3 == Ok(FailKind::Missing.text())
        && g4 == Err(DF_ENOMSG)
        && full == Err(DF_EFULL)
        && dup == Ok(1)
        && cleared == Ok(2)
        && paired
        && e.clears() == 1;

    out
}

/// 运行本卡检查。
pub fn run_unx11369_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11369");
    let l = build_ledger();

    s.add_dyn(
        "DF1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={} audit={:?}", l.len(), l.ok_count(), l.err_count(), l.audit()),
    );

    s.add(
        "DF2-NULL与报文配对",
        {
            let mut o = Opener::new();
            let _ = o.add(Image::make(0x11, 2, 0, false, false));
            let _ = o.open(0x11);
            let _ = o.open(0x12);
            o.paired() && o.nulls() == 2 && o.reports() == 2 && o.has_error()
        },
        "static",
    );

    s.add(
        "DF3-成功清残留",
        {
            let mut o = Opener::new();
            let _ = o.add(Image::make(0x11, 1, 0, false, false));
            let _ = o.open(0x12);
            let before = o.has_error();
            let h = o.open(0x11);
            before && h == Ok(1) && !o.has_error() && o.clears() == 1
        },
        "static",
    );

    s.add(
        "DF4-参数错不发布",
        {
            let mut o = Opener::new();
            let a = o.open(0);
            let b = o.take_error();
            a == Err(DF_EINVAL) && b == Err(DF_ENOMSG) && o.nulls() == 0 && o.reports() == 0
        },
        "static",
    );

    s.add(
        "DF5-五类报文互异",
        {
            let mut seen = [0u32; 5];
            let mut k = 1u32;
            let mut ok = true;
            while k <= 5 {
                let t = match FailKind::of(k) {
                    Some(f) => f.text(),
                    None => 0,
                };
                if t == 0 {
                    ok = false;
                }
                if let Some(dst) = seen.get_mut((k as usize) - 1) {
                    *dst = t;
                }
                k += 1;
            }
            let mut i = 0usize;
            while i < 5 {
                let mut j = i + 1;
                while j < 5 {
                    if seen.get(i) == seen.get(j) {
                        ok = false;
                    }
                    j += 1;
                }
                i += 1;
            }
            ok
        },
        "static",
    );

    s.add(
        "DF6-报文一次消费",
        {
            let mut o = Opener::new();
            let _ = o.open(0x77);
            let a = o.take_error();
            let b = o.take_error();
            a == Ok(FailKind::Missing.text()) && b == Err(DF_ENOMSG) && o.takes() == 1
        },
        "static",
    );

    s.add(
        "DF7-架构先于符号",
        {
            let mut o = Opener::new();
            let _ = o.add(Image::make(0x11, 2, 0, true, true));
            let a = o.open(0x11);
            let m = o.take_error();
            a == Err(DF_EBADARCH) && m == Ok(FailKind::Arch.text())
        },
        "static",
    );

    s.add(
        "DF8-依赖成环被检出",
        {
            let mut o = Opener::new();
            let _ = o.add(Image::make(0x11, 1, 0x12, false, false));
            let _ = o.add(Image::make(0x12, 1, 0x11, false, false));
            let a = o.open(0x11);
            let b = o.open(0x12);
            a == Err(DF_ECYCLE) && b == Err(DF_ECYCLE) && o.nulls() == 2
        },
        "static",
    );

    s.add(
        "DF9-非环依赖可装载",
        {
            let mut o = Opener::new();
            let _ = o.add(Image::make(0x11, 1, 0x12, false, false));
            let _ = o.add(Image::make(0x12, 1, 0, false, false));
            let a = o.open(0x11);
            a == Ok(1) && o.refs_of(0x11) == Ok(1) && o.nulls() == 0
        },
        "static",
    );

    s.add(
        "DF10-容量精确",
        {
            let mut o = Opener::new();
            let mut i = 0u32;
            while (i as usize) < CAP {
                let _ = o.add(Image::make(0x40 + i, 1, 0, false, false));
                i += 1;
            }
            let over = o.add(Image::make(0x60, 1, 0, false, false));
            let dup = o.add(Image::make(0x41, 1, 0, false, false));
            over == Err(DF_EFULL) && dup == Ok(2)
        },
        "static",
    );

    s.add(
        "DF11-账本槽序不可越",
        {
            let mut x = Ledger::new();
            let a = x.register(cell(1, OP_ADD, Ok(1)));
            let b = x.register(cell(0, OP_ADD, Ok(1)));
            let c = x.register(cell(0, OP_ADD, Ok(1)));
            a == Err(DF_EORDER) && b == Ok(()) && c == Err(DF_EORDER) && x.len() == 1
        },
        "static",
    );

    s.add(
        "DF12-观测面边界",
        {
            let x = build_ledger();
            let a = x.sample(SAMPLES - 1);
            let b = x.sample(SAMPLES);
            let mut o = Opener::new();
            let c = o.refs_of(0x11);
            let _ = o.add(Image::make(0x11, 1, 0, false, false));
            let d = o.refs_of(0x11);
            a.is_some() && b.is_none() && c == Err(DF_ENOTFOUND) && d == Ok(0)
        },
        "static",
    );

    s.add(
        "DF13-冻结重放四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            rep.is_ok()
                && one.is_ok()
                && one == rep
                && two.is_err()
                && f.is_frozen()
                && f.rejects() == 1
                && f.root() != 0
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
        let s = run_unx11369_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn success_clears_error() {
        let mut o = Opener::new();
        let _ = o.add(Image::make(0x11, 1, 0, false, false));
        assert_eq!(o.open(0x99), Err(DF_ENOTFOUND));
        assert!(o.has_error());
        assert_eq!(o.open(0x11), Ok(1));
        assert!(!o.has_error());
        assert_eq!(o.clears(), 1);
        assert_eq!(o.take_error(), Err(DF_ENOMSG));
    }

    #[test]
    fn five_messages_are_distinct() {
        let a = FailKind::Missing.text();
        let b = FailKind::Arch.text();
        let c = FailKind::Unsym.text();
        let d = FailKind::Cycle.text();
        let e = FailKind::Init.text();
        assert!(a != b && b != c && c != d && d != e);
        assert_eq!(FailKind::of(3).map(|f| f.text()), Some(c));
        assert_eq!(FailKind::of(9), None);
    }

    #[test]
    fn argument_error_publishes_nothing() {
        let mut o = Opener::new();
        assert_eq!(o.open(0), Err(DF_EINVAL));
        assert!(!o.has_error());
        assert_eq!(o.nulls(), 0);
        assert!(o.paired());
    }

    #[test]
    fn gate_order_arch_before_sym() {
        let mut o = Opener::new();
        let _ = o.add(Image::make(0x11, 2, 0, true, true));
        assert_eq!(o.open(0x11), Err(DF_EBADARCH));
        assert_eq!(o.take_error(), Ok(FailKind::Arch.text()));
    }

    #[test]
    fn cycle_detection_terminates() {
        let mut o = Opener::new();
        let _ = o.add(Image::make(0x11, 1, 0x12, false, false));
        let _ = o.add(Image::make(0x12, 1, 0x13, false, false));
        let _ = o.add(Image::make(0x13, 1, 0x11, false, false));
        assert_eq!(o.open(0x11), Err(DF_ECYCLE));
        assert_eq!(o.open(0x13), Err(DF_ECYCLE));
        assert_eq!(o.nulls(), 2);
    }

    #[test]
    fn refcount_and_capacity() {
        let mut o = Opener::new();
        let _ = o.add(Image::make(0x11, 1, 0, false, false));
        assert_eq!(o.open(0x11), Ok(1));
        assert_eq!(o.open(0x11), Ok(1));
        assert_eq!(o.refs_of(0x11), Ok(2));
        assert_eq!(o.close(1), Ok(1));
        assert_eq!(o.close(1), Ok(0));
        assert_eq!(o.close(1), Err(DF_EINVAL));
    }
}
