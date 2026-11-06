//! UNX-F11503 env 环境打印与 -i/-u 档（0xf080 段）
//!
//! `env` 的三件事各自独立：**打印**当前环境（按插入序逐行 `NAME=VALUE`）、
//! **改写**环境（`NAME=VALUE` 赋值、同名替换而非追加）、以及两个档位开关
//! ——`-i` 从空环境起算（清掉此前所有赋值），`-u NAME` 定向删除一个变量。
//!
//! 选项是**有序**的，不能先把开关摘出来再统一处理：`env -i A=1` 与
//! `env A=1 -i` 的结果不同（前者留下 A，后者清空）。因此本模型按 token 顺序
//! 逐个应用：遇到 `-i` 立刻清空已积累的赋值，遇到 `-u` 立刻删掉下一个 token
//! 指定的变量，遇到 `NAME=VALUE` 立刻写入。这个次序就是语义本身。
//!
//! 赋值闸门次序：空名或名含 '=' → 名超容 → 值超容 → 表满（同名替换不受表满
//! 限制）。名字里的 '=' 一律拒绝，因为它是切分点；值里允许 '='，只有第一个
//! '=' 是分隔符，其余属于值。
//!
//! `-u` 删除不存在的变量**不算错**（与 coreutils 一致：删除幂等），但删完之后
//! 的计数要如实回读。打印缓冲不足返回 `EBUF`，且不写半个字符：先量总长，
//! 量不下就一格不写。
//!
//! 账本十四格：八格登记成功路径（两次赋值、一次命令行赋值、一次打印、计数、
//! 一次删除、一次清空、一次两 token 计划），六格登记各类拒绝（未知选项、
//! `-u` 缺操作数、非法名、缺等号、名超容、值超容）。审计逐格复算，封印折叠
//! 全账，冻结后拒绝登记并计数。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf080;

/// 未知选项。
pub const EV_EBADOPT: u32 = SEG_BASE + 0x01;
/// `-u` 缺少操作数。
pub const EV_EOPERAND: u32 = SEG_BASE + 0x02;
/// 变量名非法（空或含 '='）。
pub const EV_EBADNAME: u32 = SEG_BASE + 0x03;
/// 赋值缺少等号。
pub const EV_ENOEQ: u32 = SEG_BASE + 0x04;
/// 变量名超容。
pub const EV_ENAMELONG: u32 = SEG_BASE + 0x05;
/// 变量值超容。
pub const EV_EVALLONG: u32 = SEG_BASE + 0x06;
/// 变量表满。
pub const EV_EFULL: u32 = SEG_BASE + 0x07;
/// 打印缓冲不足。
pub const EV_EBUF: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const EV_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const EV_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const EV_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const EV_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const EV_ENOBS: u32 = SEG_BASE + 0x0d;

/// 变量表容量。
pub const CAP: usize = 8;
/// 名字容量。
pub const NAME_CAP: usize = 32;
/// 值容量。
pub const VAL_CAP: usize = 64;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：赋值。
pub const OP_SET: u8 = 1;
/// 操作码：删除。
pub const OP_UNSET: u8 = 2;
/// 操作码：命令行 token。
pub const OP_APPLY: u8 = 3;
/// 操作码：计数。
pub const OP_COUNT: u8 = 4;
/// 操作码：打印。
pub const OP_RENDER: u8 = 5;

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

/// 一条环境变量。
#[derive(Clone, Copy)]
struct Slot {
    name: [u8; NAME_CAP],
    nlen: u8,
    val: [u8; VAL_CAP],
    vlen: u8,
}

impl Slot {
    const fn blank() -> Slot {
        Slot {
            name: [0u8; NAME_CAP],
            nlen: 0,
            val: [0u8; VAL_CAP],
            vlen: 0,
        }
    }
}

/// 定容环境表。
pub struct Env {
    slots: [Slot; CAP],
    n: usize,
}

impl Env {
    /// 空环境。
    pub const fn new() -> Env {
        Env { slots: [Slot::blank(); CAP], n: 0 }
    }

    /// 变量个数。
    pub fn count(&self) -> usize {
        self.n
    }

    /// 是否空环境。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 第 i 个变量名。
    pub fn name_at(&self, i: usize) -> Option<&[u8]> {
        let s = self.slots.get(i)?;
        s.name.get(..s.nlen as usize)
    }

    /// 第 i 个变量值。
    pub fn value_at(&self, i: usize) -> Option<&[u8]> {
        let s = self.slots.get(i)?;
        s.val.get(..s.vlen as usize)
    }

    /// 按名查下标。
    pub fn find(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                if s.nlen as usize == name.len() && s.name.get(..name.len()) == Some(name) {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 按名取值长度；不存在报 0。
    pub fn value_len(&self, name: &[u8]) -> u32 {
        match self.find(name) {
            Some(k) => match self.slots.get(k) {
                Some(s) => s.vlen as u32,
                None => 0,
            },
            None => 0,
        }
    }

    /// 清空，返回个数（恒 0）。
    pub fn clear(&mut self) -> u32 {
        self.n = 0;
        0
    }

    /// 赋值（同名替换），返回变量个数。
    pub fn set(&mut self, name: &[u8], val: &[u8]) -> Result<u32, u32> {
        if name.is_empty() {
            return Err(EV_EBADNAME);
        }
        let mut i = 0usize;
        while i < name.len() {
            if name.get(i).copied() == Some(b'=') {
                return Err(EV_EBADNAME);
            }
            i += 1;
        }
        if name.len() > NAME_CAP {
            return Err(EV_ENAMELONG);
        }
        if val.len() > VAL_CAP {
            return Err(EV_EVALLONG);
        }
        let at = match self.find(name) {
            Some(k) => k,
            None => {
                if self.n >= CAP {
                    return Err(EV_EFULL);
                }
                self.n
            }
        };
        if let Some(s) = self.slots.get_mut(at) {
            s.nlen = name.len() as u8;
            s.vlen = val.len() as u8;
            let mut k = 0usize;
            while k < NAME_CAP {
                if let Some(d) = s.name.get_mut(k) {
                    *d = 0;
                }
                k += 1;
            }
            k = 0;
            while k < VAL_CAP {
                if let Some(d) = s.val.get_mut(k) {
                    *d = 0;
                }
                k += 1;
            }
            k = 0;
            while k < name.len() {
                if let (Some(d), Some(v)) = (s.name.get_mut(k), name.get(k)) {
                    *d = *v;
                }
                k += 1;
            }
            k = 0;
            while k < val.len() {
                if let (Some(d), Some(v)) = (s.val.get_mut(k), val.get(k)) {
                    *d = *v;
                }
                k += 1;
            }
        }
        if at == self.n {
            self.n += 1;
        }
        Ok(self.n as u32)
    }

    /// 删除（幂等），返回剩余个数。
    pub fn unset(&mut self, name: &[u8]) -> Result<u32, u32> {
        if name.is_empty() {
            return Err(EV_EBADNAME);
        }
        let at = match self.find(name) {
            Some(k) => k,
            None => return Ok(self.n as u32),
        };
        let mut k = at;
        while k + 1 < self.n {
            let next = match self.slots.get(k + 1) {
                Some(v) => *v,
                None => break,
            };
            if let Some(dst) = self.slots.get_mut(k) {
                *dst = next;
            }
            k += 1;
        }
        if self.n > 0 {
            self.n -= 1;
            if let Some(last) = self.slots.get_mut(self.n) {
                *last = Slot::blank();
            }
        }
        Ok(self.n as u32)
    }

    /// 应用一个命令行 token。
    pub fn apply(&mut self, arg: &[u8]) -> Result<u32, u32> {
        if arg == b"-i" {
            return Ok(self.clear());
        }
        if arg.first().copied() == Some(b'-') {
            if arg == b"-u" {
                return Err(EV_EOPERAND);
            }
            return Err(EV_EBADOPT);
        }
        let mut eq: Option<usize> = None;
        let mut i = 0usize;
        while i < arg.len() {
            if arg.get(i).copied() == Some(b'=') {
                eq = Some(i);
                break;
            }
            i += 1;
        }
        match eq {
            Some(k) => {
                let name = match arg.get(..k) {
                    Some(v) => v,
                    None => return Err(EV_EBADNAME),
                };
                let val = match arg.get(k + 1..) {
                    Some(v) => v,
                    None => return Err(EV_EBADNAME),
                };
                self.set(name, val)
            }
            None => Err(EV_ENOEQ),
        }
    }

    /// 按序应用整条命令行。
    pub fn plan(&mut self, args: &[&[u8]]) -> Result<u32, u32> {
        let mut i = 0usize;
        while i < args.len() {
            let a = match args.get(i) {
                Some(v) => *v,
                None => break,
            };
            if a == b"-i" {
                self.clear();
            } else if a == b"-u" {
                let target = match args.get(i + 1) {
                    Some(v) => *v,
                    None => return Err(EV_EOPERAND),
                };
                let _ = self.unset(target)?;
                i += 1;
            } else if a.first().copied() == Some(b'-') {
                return Err(EV_EBADOPT);
            } else {
                let _ = self.apply(a)?;
            }
            i += 1;
        }
        Ok(self.n as u32)
    }

    /// 打印总长度。
    pub fn print_len(&self) -> usize {
        let mut need = 0usize;
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                need += s.nlen as usize + s.vlen as usize + 2;
            }
            i += 1;
        }
        need
    }

    /// 逐行打印；缓冲不足时一格不写。
    pub fn render(&self, dst: &mut [u8]) -> Result<u32, u32> {
        let need = self.print_len();
        if dst.len() < need {
            return Err(EV_EBUF);
        }
        let mut at = 0usize;
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                at = poke(dst, at, s.name.get(..s.nlen as usize));
                at = poke(dst, at, Some(b"="));
                at = poke(dst, at, s.val.get(..s.vlen as usize));
                at = poke(dst, at, Some(b"\n"));
            }
            i += 1;
        }
        Ok(at as u32)
    }
}

/// 把 `src` 写到 `dst[at..]`，返回新的写入位置。
fn poke(dst: &mut [u8], at: usize, src: Option<&[u8]>) -> usize {
    let bytes = match src {
        Some(v) => v,
        None => return at,
    };
    let mut i = 0usize;
    while i < bytes.len() {
        if let (Some(d), Some(s)) = (dst.get_mut(at + i), bytes.get(i)) {
            *d = *s;
        }
        i += 1;
    }
    at + bytes.len()
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
            return Err(EV_ESLOT);
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
        const VALS: [u32; 8] = [1, 2, 3, 37, 3, 2, 0, 2];
        const CODES: [u32; 6] =
            [EV_EBADOPT, EV_EOPERAND, EV_EBADNAME, EV_ENOEQ, EV_ENAMELONG, EV_EVALLONG];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(EV_EBAL),
            };
            if s.slot as usize != i {
                return Err(EV_ESLOT);
            }
            if s.op == 0 {
                return Err(EV_EVALUE);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(EV_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(EV_EVALUE);
                        }
                    }
                    None => return Err(EV_EBAL),
                }
                vi += 1;
            } else {
                if s.value != 0 {
                    return Err(EV_EVALUE);
                }
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(EV_EVALUE);
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

/// 十四格账本：两笔赋值 → 命令行赋值 → 打印 → 计数 → 删除 → 清空 →
/// 两 token 计划 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut e = Env::new();

    let _ = l.register(word_cell(0, OP_SET, e.set(b"PATH", b"/bin:/usr/bin")));
    let _ = l.register(word_cell(1, OP_SET, e.set(b"HOME", b"/root")));
    let _ = l.register(word_cell(2, OP_APPLY, e.apply(b"LANG=C")));

    let mut buf = [0u8; 64];
    let _ = l.register(word_cell(3, OP_RENDER, e.render(&mut buf)));
    let _ = l.register(word_cell(4, OP_COUNT, Ok(e.count() as u32)));
    let _ = l.register(word_cell(5, OP_UNSET, e.unset(b"HOME")));
    let _ = l.register(word_cell(6, OP_APPLY, e.apply(b"-i")));
    let _ = l.register(word_cell(7, OP_APPLY, e.plan(&[b"A=1", b"B=2"])));

    let _ = l.register(word_cell(8, OP_APPLY, e.plan(&[b"-x", b"A=1"])));
    let _ = l.register(word_cell(9, OP_APPLY, e.plan(&[b"-u"])));
    let _ = l.register(word_cell(10, OP_SET, e.set(b"=v", b"1")));
    let _ = l.register(word_cell(11, OP_APPLY, e.apply(b"PATH")));

    let long_name = [b'N'; NAME_CAP + 4];
    let _ = l.register(word_cell(12, OP_SET, e.set(&long_name, b"1")));

    let long_val = [b'V'; VAL_CAP + 8];
    let _ = l.register(word_cell(13, OP_SET, e.set(b"K", &long_val)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：赋值替换不增条，删除移位保持插入序。
    let mut e = Env::new();
    let a = e.set(b"A", b"1");
    let b = e.set(b"B", b"2");
    let c = e.set(b"A", b"9");
    let found = e.find(b"A");
    let vals = match (e.value_at(0), e.value_at(1)) {
        (Some(x), Some(y)) => x == b"9" && y == b"2",
        _ => false,
    };
    let removed = e.unset(b"A");
    let order = e.name_at(0) == Some(&b"B"[..]);
    out[1] = a == Ok(1)
        && b == Ok(2)
        && c == Ok(2)
        && found == Some(0)
        && vals
        && removed == Ok(1)
        && order
        && e.count() == 1;

    // 二路：选项次序决定结果。
    let mut x = Env::new();
    let seq1 = x.plan(&[b"-i", b"A=1", b"-u", b"A", b"B=2"]);
    let only_b = x.count() == 1 && x.find(b"A").is_none() && x.find(b"B") == Some(0);
    let mut y = Env::new();
    let _ = y.set(b"A", b"1");
    let seq2 = y.plan(&[b"-i"]);
    out[2] = seq1 == Ok(1)
        && only_b
        && seq2 == Ok(0)
        && y.is_empty()
        && x.plan(&[b"-i", b"C=3"]) == Ok(1)
        && x.find(b"C") == Some(0);

    // 三路：八类失败各自可辨。
    let mut z = Env::new();
    let long_name = [b'N'; NAME_CAP + 4];
    let long_val = [b'V'; VAL_CAP + 8];
    let mut small = [0u8; 2];
    let _ = z.set(b"A", b"1");
    out[3] = z.apply(b"-q") == Err(EV_EBADOPT)
        && z.plan(&[b"-u"]) == Err(EV_EOPERAND)
        && z.set(b"", b"1") == Err(EV_EBADNAME)
        && z.set(b"a=b", b"1") == Err(EV_EBADNAME)
        && z.apply(b"PATH") == Err(EV_ENOEQ)
        && z.set(&long_name, b"1") == Err(EV_ENAMELONG)
        && z.set(b"K", &long_val) == Err(EV_EVALLONG)
        && z.render(&mut small) == Err(EV_EBUF)
        && z.count() == 1;

    out
}

/// 十三项判据。
pub fn run_unx11503_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11503.env_print_opts");
    let l = build_ledger();

    s.add_dyn(
        "EV01-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "EV02--i档自空起算",
        {
            let mut e = Env::new();
            let _ = e.set(b"A", b"1");
            let _ = e.set(b"B", b"2");
            let cleared = e.apply(b"-i");
            let after = e.set(b"C", b"3");
            cleared == Ok(0) && after == Ok(1) && e.find(b"A").is_none() && e.find(b"C") == Some(0)
        },
        "static",
    );

    s.add(
        "EV03--u档定向删除",
        {
            let mut e = Env::new();
            let _ = e.set(b"A", b"1");
            let _ = e.set(b"B", b"2");
            let gone = e.plan(&[b"-u", b"A"]);
            let absent = e.plan(&[b"-u", b"ZZ"]);
            let missing = e.plan(&[b"-u"]);
            gone == Ok(1)
                && absent == Ok(1)
                && missing == Err(EV_EOPERAND)
                && e.find(b"A").is_none()
                && e.find(b"B") == Some(0)
        },
        "static",
    );

    s.add(
        "EV04-同名替换不增条",
        {
            let mut e = Env::new();
            let _ = e.set(b"PATH", b"/bin");
            let second = e.set(b"PATH", b"/usr/bin");
            second == Ok(1)
                && e.count() == 1
                && e.value_at(0) == Some(&b"/usr/bin"[..])
                && e.value_len(b"PATH") == 8
        },
        "static",
    );

    s.add(
        "EV05-名字里的等号",
        {
            let mut e = Env::new();
            let bad = e.apply(b"=v");
            let inner = e.apply(b"A=B=C");
            bad == Err(EV_EBADNAME)
                && inner == Ok(1)
                && e.value_at(0) == Some(&b"B=C"[..])
                && e.set(b"", b"x") == Err(EV_EBADNAME)
        },
        "static",
    );

    s.add(
        "EV06-裸词缺等号",
        apply_missing_eq(),
        "static",
    );

    s.add(
        "EV07-名字容量边界",
        {
            let mut e = Env::new();
            let exact = [b'N'; NAME_CAP];
            let over = [b'N'; NAME_CAP + 1];
            e.set(&exact, b"1") == Ok(1)
                && e.set(&over, b"1") == Err(EV_ENAMELONG)
                && l.sample(12).map(|x| x.code) == Some(EV_ENAMELONG)
        },
        "static",
    );

    s.add(
        "EV08-值容量边界",
        {
            let mut e = Env::new();
            let exact = [b'V'; VAL_CAP];
            let over = [b'V'; VAL_CAP + 1];
            e.set(b"K", &exact) == Ok(1)
                && e.set(b"K", &over) == Err(EV_EVALLONG)
                && l.sample(13).map(|x| x.code) == Some(EV_EVALLONG)
        },
        "static",
    );

    s.add(
        "EV09-表满边界",
        {
            let mut e = Env::new();
            let mut i = 0u8;
            while (i as usize) < CAP {
                let name = [b'K', b'A' + i];
                let _ = e.set(&name, b"x");
                i += 1;
            }
            let full = e.set(b"OVER", b"1");
            let replace = e.set(b"KA", b"y");
            full == Err(EV_EFULL)
                && replace == Ok(CAP as u32)
                && e.count() == CAP
                && e.value_at(0) == Some(&b"y"[..])
        },
        "static",
    );

    s.add(
        "EV10-打印顺序与长度",
        {
            let mut e = Env::new();
            let _ = e.set(b"PATH", b"/bin:/usr/bin");
            let _ = e.set(b"HOME", b"/root");
            let _ = e.apply(b"LANG=C");
            let mut buf = [0u8; 64];
            let n = e.render(&mut buf);
            n == Ok(37)
                && e.print_len() == 37
                && buf.get(..37) == Some(&b"PATH=/bin:/usr/bin\nHOME=/root\nLANG=C\n"[..])
                && l.sample(3).map(|x| x.value) == Some(37)
        },
        "static",
    );

    s.add(
        "EV11-打印缓冲边界",
        {
            let mut e = Env::new();
            let _ = e.set(b"A", b"1");
            let mut tiny = [0u8; 3];
            let mut exact = [0u8; 4];
            let short = e.render(&mut tiny);
            let fits = e.render(&mut exact);
            short == Err(EV_EBUF)
                && fits == Ok(4)
                && exact == [b'A', b'=', b'1', b'\n']
                && tiny == [0u8; 3]
        },
        "static",
    );

    s.add(
        "EV12-选项次序即语义",
        {
            let mut a = Env::new();
            let one = a.plan(&[b"-i", b"A=1", b"-u", b"A", b"B=2"]);
            let mut b = Env::new();
            let _ = b.set(b"A", b"1");
            let two = b.plan(&[b"A=2", b"-i"]);
            one == Ok(1)
                && a.find(b"B") == Some(0)
                && a.find(b"A").is_none()
                && two == Ok(0)
                && b.is_empty()
                && a.plan(&[b"-x"]) == Err(EV_EBADOPT)
        },
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

/// 裸词缺等号的独立判据。
fn apply_missing_eq() -> bool {
    let mut e = Env::new();
    let _ = e.set(b"PATH", b"/bin");
    let r = e.apply(b"PATH");
    r == Err(EV_ENOEQ) && e.count() == 1
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
        let s = run_unx11503_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn set_replace_unset() {
        let mut e = Env::new();
        assert_eq!(e.set(b"A", b"1"), Ok(1));
        assert_eq!(e.set(b"B", b"2"), Ok(2));
        assert_eq!(e.set(b"A", b"3"), Ok(2));
        assert_eq!(e.find(b"A"), Some(0));
        assert_eq!(e.value_at(1), Some(&b"2"[..]));
        assert_eq!(e.unset(b"A"), Ok(1));
        assert_eq!(e.name_at(0), Some(&b"B"[..]));
        assert_eq!(e.unset(b"none"), Ok(1));
        assert_eq!(e.count(), 1);
    }

    #[test]
    fn plan_order_matters() {
        let mut a = Env::new();
        assert_eq!(a.plan(&[b"-i", b"A=1", b"-u", b"A", b"B=2"]), Ok(1));
        assert_eq!(a.find(b"B"), Some(0));
        let mut b = Env::new();
        let _ = b.set(b"A", b"1");
        assert_eq!(b.plan(&[b"A=2", b"-i"]), Ok(0));
        assert!(b.is_empty());
    }

    #[test]
    fn print_and_buffer() {
        let mut e = Env::new();
        let _ = e.set(b"A", b"1");
        let mut exact = [0u8; 4];
        assert_eq!(e.render(&mut exact), Ok(4));
        assert_eq!(exact, [b'A', b'=', b'1', b'\n']);
        let mut tiny = [0u8; 3];
        assert_eq!(e.render(&mut tiny), Err(EV_EBUF));
        assert_eq!(tiny, [0u8; 3]);
    }

    #[test]
    fn capacity_limits() {
        let mut e = Env::new();
        let mut i = 0u8;
        while (i as usize) < CAP {
            assert_eq!(e.set(&[b'K', b'A' + i], b"x"), Ok(i as u32 + 1));
            i += 1;
        }
        assert_eq!(e.set(b"OVER", b"1"), Err(EV_EFULL));
        assert_eq!(e.set(b"KA", b"y"), Ok(CAP as u32));
        let over_name = [b'N'; NAME_CAP + 1];
        assert_eq!(e.set(&over_name, b"1"), Err(EV_ENAMELONG));
    }

    #[test]
    fn value_overflow() {
        let mut e = Env::new();
        let exact = [b'V'; VAL_CAP];
        assert_eq!(e.set(b"K", &exact), Ok(1));
        let over = [b'V'; VAL_CAP + 1];
        assert_eq!(e.set(b"K", &over), Err(EV_EVALLONG));
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
