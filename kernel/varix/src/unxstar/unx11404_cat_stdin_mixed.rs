//! UNX-F11404 cat stdin（`-`）与缺参语义（0xef40 段）
//!
//! `cat` 的操作数是一个**有序混合序列**：名字读文件，单独的 `-` 表示标准
//! 输入。三条语义必须同时成立：
//!
//! - **无参等价于一个 `-`**：不写任何操作数时，cat 读标准输入一次；
//! - **`-` 在参数序中的位置有效**：`cat a - b` 先读 a，再读此刻的剩余标准
//!   输入，再读 b。标准输入不是「最后再补」，而是当场消费；
//! - **标准输入是一次性流**：第一个 `-` 会把当前可读内容读到 EOF；再出现
//!   `-` 时读到的是 EOF，输出零字节。`cat - -` 不会把同一段内容输出两遍。
//!
//! 由此产生的错误类必须分开：
//!
//! - `EINVAL`：参数个数越界（超过 4）或文件名编号为 0。`-` 用 0 号 token
//!   表示，因此「0 号文件」本身是非法输入，不能与「读 stdin」混为一谈；
//! - `ENOSTDIN`：参数里有 `-`，但本进程根本没有接上标准输入通道。这与
//!   「stdin 已到 EOF」完全不同：一个是通道缺失，一个是通道读空；
//! - `ENOENT`：文件不在册；
//! - `EISDIR`/`EBUSY`/`EIO`：源不可读、被占用、读出错；
//! - `ENOSPC`：输出写满（已写部分保留）；
//! - `EFULL`：文件登记表满。
//!
//! 闸门次序（`run`）：参数个数 → 逐 token（0 走 stdin：通道 → 容量；非 0 走
//! 文件：编号 → 在册 → 目录 → 占用 → 读错）。参数个数先于一切：个数都不对，
//! 讨论某个 token 是否可读没有意义。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xef40;

/// 形状错：参数个数越界或文件名编号为 0。
pub const CS_EINVAL: u32 = SEG_BASE + 0x01;
/// 有 `-` 但没有标准输入通道。
pub const CS_ENOSTDIN: u32 = SEG_BASE + 0x02;
/// 文件不在册。
pub const CS_ENOENT: u32 = SEG_BASE + 0x03;
/// 源是目录。
pub const CS_EISDIR: u32 = SEG_BASE + 0x04;
/// 源被占用。
pub const CS_EBUSY: u32 = SEG_BASE + 0x05;
/// 源读出错。
pub const CS_EIO: u32 = SEG_BASE + 0x06;
/// 输出缓冲写满。
pub const CS_ENOSPC: u32 = SEG_BASE + 0x07;
/// 文件登记表满。
pub const CS_EFULL: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const CS_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const CS_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const CS_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const CS_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const CS_ENOBS: u32 = SEG_BASE + 0x0d;

/// 一次最多接受的参数个数。
pub const ACAP: usize = 4;
/// 文件登记表容量（即 `CAP`）。
pub const CAP: usize = 6;
/// 单件内容上限。
pub const MAXLEN: usize = 24;
/// 标准输入内容上限。
pub const ICAP: usize = 32;
/// 输出缓冲容量。
pub const OCAP: usize = 64;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// token：标准输入。
pub const TOK_STDIN: u32 = 0;

/// 种类：普通文件。
pub const KFILE: u8 = 0;
/// 种类：目录。
pub const KDIR: u8 = 1;

/// 操作码：接上标准输入。
pub const OP_ATTACH: u8 = 1;
/// 操作码：登记文件。
pub const OP_ADD: u8 = 2;
/// 操作码：按序跑一次。
pub const OP_RUN: u8 = 3;
/// 操作码：读标准输入剩余。
pub const OP_LEFT: u8 = 4;

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

/// 标准输入通道。
#[derive(Clone, Copy, Debug)]
pub struct In {
    b: [u8; ICAP],
    n: usize,
    pos: usize,
    live: bool,
}

impl In {
    /// 未接通道。
    pub const fn blank() -> In {
        In { b: [0u8; ICAP], n: 0, pos: 0, live: false }
    }

    /// 剩余字节数。
    pub fn left(&self) -> usize {
        if self.pos >= self.n {
            0
        } else {
            self.n - self.pos
        }
    }
}

/// 一个文件源。
#[derive(Clone, Copy, Debug)]
pub struct Src {
    /// 编号。
    pub id: u32,
    /// 种类。
    pub kind: u8,
    /// 内容。
    pub b: [u8; MAXLEN],
    /// 内容长度。
    pub n: usize,
    /// 是否在册。
    pub live: bool,
    /// 是否被占用。
    pub busy: bool,
    /// 是否读错。
    pub poisoned: bool,
}

impl Src {
    /// 空槽。
    pub const fn blank() -> Src {
        Src { id: 0, kind: 0, b: [0u8; MAXLEN], n: 0, live: false, busy: false, poisoned: false }
    }
}

/// 混合参数 cat 会话。
#[derive(Clone, Copy, Debug)]
pub struct CatS {
    src: [Src; CAP],
    n: usize,
    stdin: In,
    out: [u8; OCAP],
    outn: usize,
    reads: u32,
    rejects: u32,
}

impl CatS {
    /// 空会话。
    pub const fn new() -> CatS {
        CatS {
            src: [Src::blank(); CAP],
            n: 0,
            stdin: In::blank(),
            out: [0u8; OCAP],
            outn: 0,
            reads: 0,
            rejects: 0,
        }
    }

    /// 在册文件数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否没有文件。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 输出长度。
    pub fn out_len(&self) -> usize {
        self.outn
    }

    /// 标准输入剩余。
    pub fn stdin_left(&self) -> usize {
        self.stdin.left()
    }

    /// 标准输入是否已接上。
    pub fn stdin_live(&self) -> bool {
        self.stdin.live
    }

    /// 累计读源次数。
    pub fn reads(&self) -> u32 {
        self.reads
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 接上标准输入并写入内容。
    pub fn attach(&mut self, text: &str) -> Result<usize, u32> {
        if text.len() > ICAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CS_EINVAL);
        }
        let b = text.as_bytes();
        let mut i = 0usize;
        while i < b.len() {
            match b.get(i) {
                Some(c) => {
                    if let Some(d) = self.stdin.b.get_mut(i) {
                        *d = *c;
                    }
                }
                None => break,
            }
            i += 1;
        }
        self.stdin.n = b.len();
        self.stdin.pos = 0;
        self.stdin.live = true;
        Ok(b.len())
    }

    /// 登记一个文件。
    pub fn add(&mut self, id: u32, kind: u8, text: &str) -> Result<usize, u32> {
        if id == 0 || kind > KDIR || text.len() > MAXLEN {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CS_EINVAL);
        }
        if self.find(id).is_some() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CS_EBUSY);
        }
        if self.n >= CAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CS_EFULL);
        }
        let mut x = Src::blank();
        x.id = id;
        x.kind = kind;
        x.live = true;
        let b = text.as_bytes();
        let mut i = 0usize;
        while i < b.len() {
            match b.get(i) {
                Some(c) => {
                    if let Some(d) = x.b.get_mut(i) {
                        *d = *c;
                    }
                }
                None => break,
            }
            i += 1;
        }
        x.n = b.len();
        if let Some(dst) = self.src.get_mut(self.n) {
            *dst = x;
        }
        self.n += 1;
        Ok(self.n)
    }

    /// 标记占用。
    pub fn mark_busy(&mut self, id: u32) -> Result<usize, u32> {
        self.mark(id, true, false)
    }

    /// 标记读错。
    pub fn poison(&mut self, id: u32) -> Result<usize, u32> {
        self.mark(id, false, true)
    }

    fn mark(&mut self, id: u32, busy: bool, bad: bool) -> Result<usize, u32> {
        let mut i = 0usize;
        let mut at = None;
        while i < self.n {
            if let Some(s) = self.src.get(i) {
                if s.live && s.id == id {
                    at = Some(i);
                }
            }
            i += 1;
        }
        let idx = match at {
            Some(v) => v,
            None => return Err(CS_ENOENT),
        };
        let cur = match self.src.get(idx) {
            Some(v) => *v,
            None => return Err(CS_ENOENT),
        };
        if let Some(dst) = self.src.get_mut(idx) {
            *dst = Src { busy: busy || cur.busy, poisoned: bad || cur.poisoned, ..cur };
        }
        Ok(self.n)
    }

    /// 按编号找文件。
    pub fn find(&self, id: u32) -> Option<Src> {
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.src.get(i) {
                if s.live && s.id == id {
                    return Some(*s);
                }
            }
            i += 1;
        }
        None
    }

    fn push_byte(&mut self, c: u8) -> Result<(), u32> {
        if self.outn >= OCAP {
            return Err(CS_ENOSPC);
        }
        if let Some(d) = self.out.get_mut(self.outn) {
            *d = c;
        }
        self.outn += 1;
        Ok(())
    }

    /// 清空输出。
    pub fn clear(&mut self) -> usize {
        let mut i = 0usize;
        while i < self.outn {
            if let Some(d) = self.out.get_mut(i) {
                *d = 0;
            }
            i += 1;
        }
        self.outn = 0;
        self.outn
    }

    /// 读标准输入到 EOF。
    fn drain_stdin(&mut self) -> Result<usize, u32> {
        if !self.stdin.live {
            return Err(CS_ENOSTDIN);
        }
        while self.stdin.pos < self.stdin.n {
            let c = match self.stdin.b.get(self.stdin.pos) {
                Some(v) => *v,
                None => break,
            };
            self.push_byte(c)?;
            self.stdin.pos += 1;
        }
        Ok(self.outn)
    }

    /// 读一个文件。
    fn drain_file(&mut self, id: u32) -> Result<usize, u32> {
        if id == TOK_STDIN {
            return Err(CS_EINVAL);
        }
        let s = match self.find(id) {
            Some(v) => v,
            None => return Err(CS_ENOENT),
        };
        if s.kind == KDIR {
            return Err(CS_EISDIR);
        }
        if s.busy {
            return Err(CS_EBUSY);
        }
        if s.poisoned {
            return Err(CS_EIO);
        }
        let mut i = 0usize;
        while i < s.n {
            match s.b.get(i) {
                Some(c) => self.push_byte(*c)?,
                None => break,
            }
            i += 1;
        }
        Ok(self.outn)
    }

    /// 按参数序跑一次。`count == 0` 等价于一个 `-`。
    pub fn run(&mut self, tokens: [u32; ACAP], count: usize) -> Result<usize, u32> {
        if count > ACAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CS_EINVAL);
        }
        let _ = self.clear();
        if count == 0 {
            let r = self.drain_stdin();
            if r.is_err() {
                self.rejects = self.rejects.saturating_add(1);
                return r;
            }
            self.reads = self.reads.saturating_add(1);
            return Ok(self.outn);
        }
        let mut i = 0usize;
        while i < count {
            let tok = match tokens.get(i) {
                Some(v) => *v,
                None => return Err(CS_EINVAL),
            };
            let r = if tok == TOK_STDIN {
                self.drain_stdin()
            } else {
                self.drain_file(tok)
            };
            if r.is_err() {
                self.rejects = self.rejects.saturating_add(1);
                return r.map(|_| self.outn);
            }
            self.reads = self.reads.saturating_add(1);
            i += 1;
        }
        Ok(self.outn)
    }

    /// 输出是否等于字面量。
    pub fn out_eq(&self, s: &str) -> bool {
        let b = s.as_bytes();
        if self.outn != b.len() {
            return false;
        }
        let mut i = 0usize;
        while i < self.outn {
            let a = match self.out.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let c = match b.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if a != c {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 输出指纹。
    pub fn digest(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.outn {
            if let Some(c) = self.out.get(i) {
                h = fold(h, *c as u64);
            }
            i += 1;
        }
        h
    }

    /// 不变式：文件表、输出、stdin 位置自洽。
    pub fn consistent(&self) -> bool {
        if self.n > CAP || self.outn > OCAP || self.stdin.pos > self.stdin.n {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            let a = match self.src.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if !a.live || a.id == 0 || a.n > MAXLEN || a.kind > KDIR {
                return false;
            }
            let mut j = i + 1;
            while j < self.n {
                if let Some(b) = self.src.get(j) {
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
            return Err(CS_ESLOT);
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
            return Err(CS_EBAL);
        }
        const VALS: [u32; 8] = [3, 1, 2, 7, 2, 0, 0, 4];
        const CODES: [u32; 6] = [CS_EINVAL, CS_ENOSTDIN, CS_ENOENT, CS_EISDIR, CS_EBUSY, CS_EIO];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(CS_EBAL),
            };
            if s.slot as usize != i {
                return Err(CS_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(CS_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(CS_EVALUE);
                        }
                    }
                    None => return Err(CS_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(CS_EVALUE);
                        }
                    }
                    None => return Err(CS_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(CS_EBAL);
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
            return Err(CS_EBAL);
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
            Err(CS_ESEAL)
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

/// 十四格账本：接 stdin + 两件 + 混合跑 + 再跑 + 无参 + 剩余 + 双件 + 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut c = CatS::new();

    let _ = l.register(cell(0, OP_ATTACH, c.attach("STD")));
    let _ = l.register(cell(1, OP_ADD, c.add(1, KFILE, "aa")));
    let _ = l.register(cell(2, OP_ADD, c.add(2, KFILE, "bb")));
    let _ = l.register(cell(3, OP_RUN, c.run([1, TOK_STDIN, 2, 0], 3)));
    let _ = l.register(cell(4, OP_RUN, c.run([TOK_STDIN, 1, 0, 0], 2)));
    let _ = l.register(cell(5, OP_RUN, c.run([0; ACAP], 0)));
    let _ = l.register(word_cell(6, OP_LEFT, Ok(c.stdin_left() as u32)));
    let _ = l.register(cell(7, OP_RUN, c.run([2, 2, 0, 0], 2)));

    let mut a = CatS::new();
    let _ = l.register(cell(8, OP_RUN, a.run([1, 2, 3, 4], ACAP + 1)));

    let mut b = CatS::new();
    let _ = l.register(cell(9, OP_RUN, b.run([TOK_STDIN, 0, 0, 0], 1)));

    let mut d = CatS::new();
    let _ = l.register(cell(10, OP_RUN, d.run([9, 0, 0, 0], 1)));

    let mut e = CatS::new();
    let _ = e.add(1, KDIR, "");
    let _ = l.register(cell(11, OP_RUN, e.run([1, 0, 0, 0], 1)));

    let mut f = CatS::new();
    let _ = f.add(1, KFILE, "x");
    let _ = f.mark_busy(1);
    let _ = l.register(cell(12, OP_RUN, f.run([1, 0, 0, 0], 1)));

    let mut g = CatS::new();
    let _ = g.add(1, KFILE, "x");
    let _ = g.poison(1);
    let _ = l.register(cell(13, OP_RUN, g.run([1, 0, 0, 0], 1)));

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

    // 二路：混合序、无参、二次 stdin 空读。
    let mut c = CatS::new();
    let _ = c.attach("STD");
    let _ = c.add(1, KFILE, "aa");
    let _ = c.add(2, KFILE, "bb");
    let mixed = c.run([1, TOK_STDIN, 2, 0], 3);
    let mixed_ok = mixed == Ok(7) && c.out_eq("aaSTDbb") && c.stdin_left() == 0;
    let again = c.run([TOK_STDIN, 0, 0, 0], 1);
    let twice = c.run([TOK_STDIN, TOK_STDIN, 0, 0], 2);
    out[1] = mixed_ok
        && again == Ok(0)
        && twice == Ok(0)
        && c.stdin_live()
        && c.reads() == 6
        && c.consistent()
        && {
            let mut d = CatS::new();
            let _ = d.attach("hi");
            d.run([0; ACAP], 0) == Ok(2) && d.out_eq("hi") && d.stdin_left() == 0
        };

    // 三路：六类问题彼此可辨。
    let mut a = CatS::new();
    let shape = a.run([0; ACAP], ACAP + 1) == Err(CS_EINVAL);
    let nostdin = a.run([TOK_STDIN, 0, 0, 0], 1) == Err(CS_ENOSTDIN);
    let miss = a.run([9, 0, 0, 0], 1) == Err(CS_ENOENT);
    let _ = a.add(1, KDIR, "");
    let dir = a.run([1, 0, 0, 0], 1) == Err(CS_EISDIR);
    let _ = a.add(2, KFILE, "z");
    let _ = a.mark_busy(2);
    let busy = a.run([2, 0, 0, 0], 1) == Err(CS_EBUSY);
    let _ = a.add(3, KFILE, "w");
    let _ = a.poison(3);
    let io = a.run([3, 0, 0, 0], 1) == Err(CS_EIO);
    out[2] = shape && nostdin && miss && dir && busy && io && a.consistent();

    // 四路：闸门次序、容量与输出上限。
    let mut q = CatS::new();
    let g1 = q.run([0; ACAP], ACAP + 1) == Err(CS_EINVAL);
    let g2 = q.run([TOK_STDIN, 0, 0, 0], 1) == Err(CS_ENOSTDIN);
    let _ = q.run([9, 0, 0, 0], 1);
    let _ = q.attach("");
    let g3 = q.run([TOK_STDIN, 0, 0, 0], 1) == Ok(0);
    let mut full = CatS::new();
    let mut i = 1u32;
    while full.len() < CAP {
        let _ = full.add(i, KFILE, "q");
        i += 1;
    }
    let g4 = full.add(99, KFILE, "q") == Err(CS_EFULL);
    let mut big = CatS::new();
    let _ = big.add(1, KFILE, "abcdefghijklmnopqrstuvwx");
    let _ = big.add(2, KFILE, "abcdefghijklmnopqrstuvwx");
    let _ = big.add(3, KFILE, "abcdefghijklmnopqrstuvwx");
    let g5 = big.run([1, 2, 0, 0], 2) == Ok(48);
    let g6 = big.run([1, 2, 3, 0], 3) == Err(CS_ENOSPC) && big.out_len() == OCAP;
    out[3] = g1 && g2 && g3 && g4 && g5 && g6 && q.consistent() && full.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11404_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11404.cat_stdin_mixed");
    let l = build_ledger();

    s.add_dyn(
        "CS1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "CS2-无参即读stdin",
        l.sample(5).map(|x| x.value) == Some(0)
            && {
                let mut c = CatS::new();
                let _ = c.attach("hi");
                c.run([0; ACAP], 0) == Ok(2) && c.out_eq("hi") && c.reads() == 1
            },
        "static",
    );

    s.add(
        "CS3-混排序当场消费",
        l.sample(3).map(|x| x.value) == Some(7)
            && {
                let mut c = CatS::new();
                let _ = c.attach("STD");
                let _ = c.add(1, KFILE, "aa");
                let _ = c.add(2, KFILE, "bb");
                c.run([1, TOK_STDIN, 2, 0], 3) == Ok(7) && c.out_eq("aaSTDbb")
            },
        "static",
    );

    s.add(
        "CS4-二次stdin读到EOF",
        l.sample(4).map(|x| x.value) == Some(2)
            && {
                let mut c = CatS::new();
                let _ = c.attach("x");
                let first = c.run([TOK_STDIN, 0, 0, 0], 1) == Ok(1);
                let second = c.run([TOK_STDIN, TOK_STDIN, 0, 0], 2) == Ok(0);
                first && second && c.stdin_left() == 0 && c.out_eq("")
            },
        "static",
    );

    s.add(
        "CS5-未接stdin时段",
        l.sample(9).map(|x| x.code) == Some(CS_ENOSTDIN)
            && {
                let mut c = CatS::new();
                c.run([TOK_STDIN, 0, 0, 0], 1) == Err(CS_ENOSTDIN)
                    && c.stdin_live() == false
                    && c.out_len() == 0
            },
        "static",
    );

    s.add(
        "CS6-参数个数EINVAL",
        l.sample(8).map(|x| x.code) == Some(CS_EINVAL)
            && {
                let mut c = CatS::new();
                c.run([0; ACAP], ACAP + 1) == Err(CS_EINVAL)
                    && c.add(0, KFILE, "x") == Err(CS_EINVAL)
                    && c.len() == 0
            },
        "static",
    );

    s.add(
        "CS7-缺件ENOENT",
        l.sample(10).map(|x| x.code) == Some(CS_ENOENT)
            && {
                let mut c = CatS::new();
                c.run([9, 0, 0, 0], 1) == Err(CS_ENOENT) && c.out_len() == 0
            },
        "static",
    );

    s.add(
        "CS8-目录EISDIR",
        l.sample(11).map(|x| x.code) == Some(CS_EISDIR)
            && {
                let mut c = CatS::new();
                let _ = c.add(1, KDIR, "");
                c.run([1, 0, 0, 0], 1) == Err(CS_EISDIR)
            },
        "static",
    );

    s.add(
        "CS9-占用EBUSY",
        l.sample(12).map(|x| x.code) == Some(CS_EBUSY)
            && {
                let mut c = CatS::new();
                let _ = c.add(1, KFILE, "x");
                let _ = c.mark_busy(1);
                c.run([1, 0, 0, 0], 1) == Err(CS_EBUSY) && c.out_len() == 0
            },
        "static",
    );

    s.add(
        "CS10-读错EIO",
        l.sample(13).map(|x| x.code) == Some(CS_EIO)
            && {
                let mut c = CatS::new();
                let _ = c.add(1, KFILE, "x");
                let _ = c.add(2, KFILE, "y");
                let _ = c.poison(1);
                c.run([1, 0, 0, 0], 1) == Err(CS_EIO) && c.run([2, 0, 0, 0], 1) == Ok(1)
            },
        "static",
    );

    s.add(
        "CS11-输出满ENOSPC",
        {
            let mut c = CatS::new();
            let _ = c.add(1, KFILE, "abcdefghijklmnopqrstuvwx");
            let _ = c.add(2, KFILE, "abcdefghijklmnopqrstuvwx");
            let _ = c.add(3, KFILE, "abcdefghijklmnopqrstuvwx");
            c.run([1, 2, 3, 0], 3) == Err(CS_ENOSPC)
                && c.out_len() == OCAP
                && c.consistent()
        },
        "static",
    );

    s.add(
        "CS12-登记满EFULL",
        {
            let mut c = CatS::new();
            let mut i = 1u32;
            while c.len() < CAP {
                let _ = c.add(i, KFILE, "q");
                i += 1;
            }
            c.add(9, KFILE, "q") == Err(CS_EFULL) && c.len() == CAP && c.consistent()
        },
        "static",
    );

    s.add(
        "CS13-冻结重放四注入",
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
        let s = run_unx11404_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn no_args_reads_stdin_once() {
        let mut c = CatS::new();
        assert_eq!(c.attach("hi"), Ok(2));
        assert_eq!(c.run([0; ACAP], 0), Ok(2));
        assert!(c.out_eq("hi"));
        assert_eq!(c.stdin_left(), 0);
        assert_eq!(c.run([0; ACAP], 0), Ok(0));
    }

    #[test]
    fn mixed_order_is_respected() {
        let mut c = CatS::new();
        let _ = c.attach("S");
        let _ = c.add(1, KFILE, "a");
        let _ = c.add(2, KFILE, "b");
        assert_eq!(c.run([1, TOK_STDIN, 2, 0], 3), Ok(3));
        assert!(c.out_eq("aSb"));
        assert_eq!(c.run([2, 1, TOK_STDIN, 0], 3), Ok(2));
        assert!(c.out_eq("ba"));
    }

    #[test]
    fn second_dash_is_empty() {
        let mut c = CatS::new();
        let _ = c.attach("xyz");
        assert_eq!(c.run([TOK_STDIN, TOK_STDIN, 0, 0], 2), Ok(3));
        assert!(c.out_eq("xyz"));
        assert_eq!(c.stdin_left(), 0);
    }

    #[test]
    fn missing_stdin_is_not_eof() {
        let mut c = CatS::new();
        assert_eq!(c.run([TOK_STDIN, 0, 0, 0], 1), Err(CS_ENOSTDIN));
        let _ = c.attach("");
        assert_eq!(c.run([TOK_STDIN, 0, 0, 0], 1), Ok(0));
    }

    #[test]
    fn gate_order_is_stable() {
        let mut c = CatS::new();
        assert_eq!(c.run([0; ACAP], ACAP + 1), Err(CS_EINVAL));
        assert_eq!(c.run([TOK_STDIN, 0, 0, 0], 1), Err(CS_ENOSTDIN));
        assert_eq!(c.run([9, 0, 0, 0], 1), Err(CS_ENOENT));
        let _ = c.add(1, KDIR, "");
        assert_eq!(c.run([1, 0, 0, 0], 1), Err(CS_EISDIR));
    }

    #[test]
    fn inject_four_is_all_true() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
