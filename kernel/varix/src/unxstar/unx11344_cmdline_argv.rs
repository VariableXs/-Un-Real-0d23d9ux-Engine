//! UNX-F11344 /proc/self/cmdline 语义（0xeb80 段）
//!
//! `/proc/self/cmdline` 不是「一行命令」，而是把进程的 argv 字符串区
//! 原样倒出来的字节流：**每个参数后跟一个 NUL，最后一个参数也有**。
//! 这三条性质决定了所有解析器的写法：
//!
//! 1. 总长度 = `Σ(len(arg_i) + 1)`。空 argv 的总长度为 0，读出来是空文件；
//! 2. 结尾一定有一个 NUL。按 `\n` 切分、或把最后一次读取当字符串用，都会
//!    把尾巴读成未定义内容；
//! 3. 参数内部不可能出现 NUL——C 字符串在 NUL 处结束。若发现内嵌 NUL，
//!    说明调用方给的不是合法 argv，必须在构造阶段拒绝，而不是让读到一半的
//!    解析器自行处理。
//!
//! proc 文件还支持**带偏移的短读**：`read(fd, buf, n)` 每次最多给 n 字节，
//! 偏移等于总长时返回 0（EOF），超过总长才是错误。把「恰好读完」和「越过
//! 末尾」混为一谈，是 `/proc/self/cmdline` 消费者最常见的 off-by-one。
//!
//! 闸门次序（构造）：参数个数 → 单参数长度 → 内嵌 NUL。个数是 O(1) 的
//! 边界检查，先拒绝超大向量；长度是逐参数上界；NUL 扫描最贵，放最后。
//! 校验全部通过后才落盘，失败不破坏原参数表。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xeb80;

/// 参数内嵌 NUL。
pub const CM_EEMBED: u32 = SEG_BASE + 0x01;
/// 单参数超长。
pub const CM_EARG: u32 = SEG_BASE + 0x02;
/// 参数个数超限。
pub const CM_EARGC: u32 = SEG_BASE + 0x03;
/// 读取偏移越过末尾。
pub const CM_EOFF: u32 = SEG_BASE + 0x04;
/// 参数下标越界。
pub const CM_EINDEX: u32 = SEG_BASE + 0x05;
/// 参数表为空。
pub const CM_EEMPTY: u32 = SEG_BASE + 0x06;
/// 字节下标越过末尾。
pub const CM_ENOBS: u32 = SEG_BASE + 0x07;
/// 账本失衡。
pub const CM_EBAL: u32 = SEG_BASE + 0x08;
/// 账本槽位乱序。
pub const CM_EORDER: u32 = SEG_BASE + 0x09;
/// 观测值与复算不符。
pub const CM_EMISMATCH: u32 = SEG_BASE + 0x0a;
/// 冻结后重放不一致。
pub const CM_ESEAL: u32 = SEG_BASE + 0x0b;

/// 参数个数上限。
pub const MAX_ARGS: usize = 6;
/// 单参数字节上限。
pub const ARG_CAP: usize = 16;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：构造。
pub const OP_SET: u8 = 1;
/// 操作码：总长。
pub const OP_TOTAL: u8 = 2;
/// 操作码：参数个数。
pub const OP_ARGC: u8 = 3;
/// 操作码：参数长度。
pub const OP_ARGLEN: u8 = 4;
/// 操作码：带偏移读取。
pub const OP_READ: u8 = 5;
/// 操作码：取字节。
pub const OP_BYTE: u8 = 6;
/// 操作码：终止符位置。
pub const OP_NUL: u8 = 7;
/// 操作码：首参数。
pub const OP_ARGV0: u8 = 8;

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

/// argv 参数表。
#[derive(Clone, Copy, Debug)]
pub struct Cmdline {
    slots: [[u8; ARG_CAP]; MAX_ARGS],
    lens: [usize; MAX_ARGS],
    argc: usize,
    total: usize,
}

impl Cmdline {
    /// 空参数表。
    pub const fn new() -> Cmdline {
        Cmdline {
            slots: [[0u8; ARG_CAP]; MAX_ARGS],
            lens: [0usize; MAX_ARGS],
            argc: 0,
            total: 0,
        }
    }

    /// 构造参数表。
    ///
    /// 成功返回 `/proc/self/cmdline` 的总字节数（含全部结尾 NUL）。
    pub fn set(&mut self, args: &[&[u8]]) -> Result<u32, u32> {
        if args.len() > MAX_ARGS {
            return Err(CM_EARGC);
        }
        let mut i = 0usize;
        while i < args.len() {
            match args.get(i) {
                Some(a) => {
                    if a.len() > ARG_CAP {
                        return Err(CM_EARG);
                    }
                }
                None => return Err(CM_EARGC),
            }
            i += 1;
        }
        // 长度全部合格后才扫描内嵌 NUL。
        i = 0;
        while i < args.len() {
            match args.get(i) {
                Some(a) => {
                    let mut k = 0usize;
                    while k < a.len() {
                        if let Some(b) = a.get(k) {
                            if *b == 0 {
                                return Err(CM_EEMBED);
                            }
                        }
                        k += 1;
                    }
                }
                None => return Err(CM_EARGC),
            }
            i += 1;
        }

        let mut total = 0usize;
        i = 0;
        while i < args.len() {
            match args.get(i) {
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
                    total += a.len() + 1;
                }
                None => return Err(CM_EARGC),
            }
            i += 1;
        }
        self.argc = args.len();
        self.total = total;
        Ok(total as u32)
    }

    /// 参数个数。
    pub fn argc(&self) -> usize {
        self.argc
    }

    /// 参数表是否为空。
    pub fn is_empty(&self) -> bool {
        self.argc == 0
    }

    /// 总字节数（含结尾 NUL）。
    pub fn total(&self) -> usize {
        self.total
    }

    /// 第 i 个参数。
    pub fn arg(&self, i: usize) -> Result<&[u8], u32> {
        if i >= self.argc {
            return Err(CM_EINDEX);
        }
        let l = match self.lens.get(i) {
            Some(v) => *v,
            None => return Err(CM_EINDEX),
        };
        match self.slots.get(i) {
            Some(row) => match row.get(..l) {
                Some(s) => Ok(s),
                None => Err(CM_EINDEX),
            },
            None => Err(CM_EINDEX),
        }
    }

    /// argv[0]；空参数表报 `EEMPTY`。
    pub fn argv0(&self) -> Result<&[u8], u32> {
        if self.argc == 0 {
            return Err(CM_EEMPTY);
        }
        self.arg(0)
    }

    /// 第 i 个参数之后那个 NUL 的绝对偏移。
    pub fn nul_at(&self, i: usize) -> Result<u32, u32> {
        if i >= self.argc {
            return Err(CM_EINDEX);
        }
        let mut off = 0usize;
        let mut k = 0usize;
        while k <= i {
            match self.lens.get(k) {
                Some(l) => off += *l + 1,
                None => return Err(CM_EINDEX),
            }
            k += 1;
        }
        // off 是第 i 个 NUL 之后的位置，减一即 NUL 本身。
        Ok((off - 1) as u32)
    }

    /// 字节流中第 i 个字节。
    ///
    /// 参数的结尾 NUL 属于字节流，能取到 0；越过总长报 `ENOBS`。
    pub fn byte_at(&self, i: usize) -> Result<u32, u32> {
        if i >= self.total {
            return Err(CM_ENOBS);
        }
        let mut off = 0usize;
        let mut a = 0usize;
        while a < self.argc {
            let l = match self.lens.get(a) {
                Some(v) => *v,
                None => return Err(CM_ENOBS),
            };
            if i < off + l + 1 {
                let rel = i - off;
                if rel == l {
                    return Ok(0);
                }
                return match self.slots.get(a).and_then(|row| row.get(rel)) {
                    Some(b) => Ok(*b as u32),
                    None => Err(CM_ENOBS),
                };
            }
            off += l + 1;
            a += 1;
        }
        Err(CM_ENOBS)
    }

    /// 模拟带偏移的 `read(2)`。
    ///
    /// 偏移等于总长返回 0（EOF），大于总长报 `EOFF`；返回值不超过剩余量。
    pub fn read_from(&self, off: usize, size: usize) -> Result<u32, u32> {
        if off > self.total {
            return Err(CM_EOFF);
        }
        let avail = self.total - off;
        let n = if size < avail { size } else { avail };
        Ok(n as u32)
    }

    /// 尾部 NUL 是否在位。
    pub fn ends_with_nul(&self) -> bool {
        if self.argc == 0 {
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
            return Err(CM_EORDER);
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
            return Err(CM_EBAL);
        }
        const VALS: [u32; 8] = [23, 3, 2, 23, 6, 3, 8, 0];
        const CODES: [u32; 6] = [
            CM_EEMBED,
            CM_EARG,
            CM_EARGC,
            CM_EOFF,
            CM_EINDEX,
            CM_EEMPTY,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(CM_EBAL),
            };
            if s.slot as usize != i {
                return Err(CM_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(CM_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(CM_EMISMATCH);
                        }
                    }
                    None => return Err(CM_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(CM_EMISMATCH);
                        }
                    }
                    None => return Err(CM_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(CM_EBAL);
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
            return Err(CM_EBAL);
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
            Err(CM_ESEAL)
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

/// 十四格账本：三参数布局 → 长度/读取 → 终止符 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut c = Cmdline::new();

    let argv: [&[u8]; 3] = [b"vxbox", b"-s", b"console=ttyS0"];
    let _ = l.register(word_cell(0, OP_SET, c.set(&argv)));
    let _ = l.register(cell(1, OP_ARGC, Ok(c.argc())));
    let _ = l.register(cell(2, OP_ARGLEN, c.arg(1).map(|a| a.len())));
    let _ = l.register(cell(3, OP_READ, c.read_from(0, 23).map(|n| n as usize)));
    let _ = l.register(cell(4, OP_READ, c.read_from(0, 6).map(|n| n as usize)));
    let _ = l.register(cell(5, OP_READ, c.read_from(6, 3).map(|n| n as usize)));
    let _ = l.register(word_cell(6, OP_NUL, c.nul_at(1)));
    let _ = l.register(cell(7, OP_READ, c.read_from(23, 4).map(|n| n as usize)));

    let embedded: [&[u8]; 1] = [b"a\0b"];
    let _ = l.register(word_cell(8, OP_SET, c.set(&embedded)));

    let long: [&[u8]; 1] = [b"0123456789abcdefg"];
    let _ = l.register(word_cell(9, OP_SET, c.set(&long)));

    let seven: [&[u8]; 7] = [b"a", b"b", b"c", b"d", b"e", b"f", b"g"];
    let _ = l.register(word_cell(10, OP_SET, c.set(&seven)));

    let _ = l.register(cell(11, OP_READ, c.read_from(24, 1).map(|n| n as usize)));
    let _ = l.register(cell(12, OP_ARGLEN, c.arg(9).map(|a| a.len())));

    let empty = Cmdline::new();
    let _ = l.register(cell(13, OP_ARGV0, empty.argv0().map(|a| a.len())));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：NUL 联结布局。
    let argv: [&[u8]; 3] = [b"vxbox", b"-s", b"console=ttyS0"];
    let mut c = Cmdline::new();
    let set = c.set(&argv);
    out[1] = set == Ok(23)
        && c.total() == 23
        && c.argc() == 3
        && c.byte_at(0) == Ok(b'v' as u32)
        && c.byte_at(4) == Ok(b'x' as u32)
        && c.byte_at(5) == Ok(0)
        && c.byte_at(6) == Ok(b'-' as u32)
        && c.byte_at(7) == Ok(b's' as u32)
        && c.byte_at(8) == Ok(0)
        && c.byte_at(9) == Ok(b'c' as u32)
        && c.byte_at(21) == Ok(b'0' as u32)
        && c.byte_at(22) == Ok(0)
        && c.byte_at(23) == Err(CM_ENOBS)
        && c.ends_with_nul()
        && c.nul_at(0) == Ok(5)
        && c.nul_at(1) == Ok(8)
        && c.nul_at(2) == Ok(22);

    // 二路：带偏移短读与边界。
    let mut d = Cmdline::new();
    let _ = d.set(&argv);
    out[2] = d.read_from(0, 23) == Ok(23)
        && d.read_from(0, 100) == Ok(23)
        && d.read_from(4, 4) == Ok(4)
        && d.read_from(8, 2) == Ok(2)
        && d.read_from(9, 14) == Ok(14)
        && d.read_from(9, 20) == Ok(14)
        && d.read_from(22, 1) == Ok(1)
        && d.read_from(23, 0) == Ok(0)
        && d.read_from(23, 8) == Ok(0)
        && d.read_from(24, 1) == Err(CM_EOFF)
        && d.arg(0) == Ok(&b"vxbox"[..])
        && d.arg(1) == Ok(&b"-s"[..])
        && d.arg(2) == Ok(&b"console=ttyS0"[..])
        && d.arg(3) == Err(CM_EINDEX);

    // 三路：闸门次序、失败不改状态、空参数表。
    let mut e = Cmdline::new();
    let _ = e.set(&argv);
    let oversized: [&[u8]; 7] = [b"a", b"b", b"c", b"d", b"e", b"f", b"g"];
    let long: [&[u8]; 1] = [b"0123456789abcdefg"];
    let embedded: [&[u8]; 1] = [b"a\0b"];
    let mixed: [&[u8]; 7] = [b"a", b"b", b"c", b"d", b"e", b"f", b"0123456789abcdefg"];
    let count_first = e.set(&mixed).is_err() && e.set(&oversized).is_err();
    let count_code = e.set(&oversized) == Err(CM_EARGC);
    let long_code = e.set(&long) == Err(CM_EARG);
    let embed_code = e.set(&embedded) == Err(CM_EEMBED);
    let preserved = e.argc() == 3
        && e.arg(0) == Ok(&b"vxbox"[..])
        && e.total() == 23
        && e.read_from(0, 23) == Ok(23);

    let mut z = Cmdline::new();
    let empty_set = z.set(&[]) == Ok(0);
    let empty_state = z.argc() == 0
        && z.is_empty()
        && z.total() == 0
        && z.argv0() == Err(CM_EEMPTY)
        && z.arg(0) == Err(CM_EINDEX)
        && z.nul_at(0) == Err(CM_EINDEX)
        && z.read_from(0, 8) == Ok(0)
        && z.read_from(1, 1) == Err(CM_EOFF)
        && z.ends_with_nul();
    out[3] = count_first
        && count_code
        && long_code
        && embed_code
        && preserved
        && empty_set
        && empty_state;

    out
}

/// 十三项判据。
pub fn run_unx11344_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11344.cmdline_argv");
    let l = build_ledger();

    s.add_dyn(
        "CL1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "CL2-总字节数含结尾NUL",
        l.sample(0).map(|x| x.value) == Some(23),
        "static",
    );

    s.add(
        "CL3-参数个数与长度",
        l.sample(1).map(|x| x.value) == Some(3)
            && l.sample(2).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "CL4-完整读取与尾NUL",
        {
            let argv: [&[u8]; 3] = [b"vxbox", b"-s", b"console=ttyS0"];
            let mut c = Cmdline::new();
            let _ = c.set(&argv);
            l.sample(3).map(|x| x.value) == Some(23)
                && c.read_from(0, 23) == Ok(23)
                && c.byte_at(22) == Ok(0)
                && c.byte_at(21) == Ok(b'0' as u32)
        },
        "static",
    );

    s.add(
        "CL5-首段读取",
        l.sample(4).map(|x| x.value) == Some(6),
        "static",
    );

    s.add(
        "CL6-中段读取与EOF",
        l.sample(5).map(|x| x.value) == Some(3)
            && l.sample(7).map(|x| x.value) == Some(0),
        "static",
    );

    s.add(
        "CL7-首参数终止符位置",
        {
            let argv: [&[u8]; 3] = [b"vxbox", b"-s", b"console=ttyS0"];
            let mut c = Cmdline::new();
            let _ = c.set(&argv);
            l.sample(6).map(|x| x.value) == Some(8)
                && c.byte_at(8) == Ok(0)
                && c.byte_at(9) == Ok(b'c' as u32)
                && c.nul_at(2) == Ok(22)
        },
        "static",
    );

    s.add(
        "CL8-内嵌NUL拒绝",
        l.sample(8).map(|x| x.code) == Some(CM_EEMBED),
        "static",
    );

    s.add(
        "CL9-超长参数拒绝",
        l.sample(9).map(|x| x.code) == Some(CM_EARG),
        "static",
    );

    s.add(
        "CL10-参数过多拒绝",
        l.sample(10).map(|x| x.code) == Some(CM_EARGC),
        "static",
    );

    s.add(
        "CL11-越界读取与下标",
        l.sample(11).map(|x| x.code) == Some(CM_EOFF)
            && l.sample(12).map(|x| x.code) == Some(CM_EINDEX),
        "static",
    );

    s.add(
        "CL12-空参数表语义",
        l.sample(13).map(|x| x.code) == Some(CM_EEMPTY)
            && {
                let z = Cmdline::new();
                z.is_empty()
                    && z.total() == 0
                    && z.read_from(0, 4) == Ok(0)
                    && z.read_from(1, 4) == Err(CM_EOFF)
                    && z.ends_with_nul()
            },
        "static",
    );

    s.add(
        "CL13-冻结重放四注入",
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

    fn three() -> Cmdline {
        let argv: [&[u8]; 3] = [b"vxbox", b"-s", b"console=ttyS0"];
        let mut c = Cmdline::new();
        let _ = c.set(&argv);
        c
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
        let s = run_unx11344_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn nul_layout() {
        let c = three();
        assert_eq!(c.total(), 23);
        assert_eq!(c.argc(), 3);
        assert_eq!(c.nul_at(0), Ok(5));
        assert_eq!(c.nul_at(1), Ok(8));
        assert_eq!(c.nul_at(2), Ok(22));
        assert_eq!(c.byte_at(5), Ok(0));
        assert_eq!(c.byte_at(8), Ok(0));
        assert_eq!(c.byte_at(22), Ok(0));
        assert_eq!(c.byte_at(23), Err(CM_ENOBS));
        assert!(c.ends_with_nul());
    }

    #[test]
    fn partial_reads() {
        let c = three();
        assert_eq!(c.read_from(0, 6), Ok(6));
        assert_eq!(c.read_from(6, 3), Ok(3));
        assert_eq!(c.read_from(9, 20), Ok(14));
        assert_eq!(c.read_from(23, 4), Ok(0));
        assert_eq!(c.read_from(24, 4), Err(CM_EOFF));
    }

    #[test]
    fn args_are_slices() {
        let c = three();
        assert_eq!(c.arg(0), Ok(&b"vxbox"[..]));
        assert_eq!(c.arg(1), Ok(&b"-s"[..]));
        assert_eq!(c.arg(2), Ok(&b"console=ttyS0"[..]));
        assert_eq!(c.arg(3), Err(CM_EINDEX));
        assert_eq!(c.argv0(), Ok(&b"vxbox"[..]));
    }

    #[test]
    fn empty_cmdline() {
        let mut c = Cmdline::new();
        assert_eq!(c.set(&[]), Ok(0));
        assert!(c.is_empty());
        assert_eq!(c.argv0(), Err(CM_EEMPTY));
        assert_eq!(c.read_from(0, 8), Ok(0));
        assert_eq!(c.read_from(1, 8), Err(CM_EOFF));
        assert!(c.ends_with_nul());
    }

    #[test]
    fn gate_priority_and_atomicity() {
        let mut c = three();
        let seven: [&[u8]; 7] = [b"a", b"b", b"c", b"d", b"e", b"f", b"g"];
        let long: [&[u8]; 1] = [b"0123456789abcdefg"];
        let embedded: [&[u8]; 1] = [b"a\0b"];
        assert_eq!(c.set(&seven), Err(CM_EARGC));
        assert_eq!(c.set(&long), Err(CM_EARG));
        assert_eq!(c.set(&embedded), Err(CM_EEMBED));
        assert_eq!(c.argc(), 3);
        assert_eq!(c.arg(2), Ok(&b"console=ttyS0"[..]));
    }

    #[test]
    fn capacity_is_exact() {
        let exact: [&[u8]; 1] = [b"0123456789abcdef"];
        let mut c = Cmdline::new();
        assert_eq!(c.set(&exact), Ok(17));
        assert_eq!(c.arg(0).map(|a| a.len()), Ok(ARG_CAP));
        let six: [&[u8]; 6] = [b"a", b"b", b"c", b"d", b"e", b"f"];
        assert_eq!(c.set(&six), Ok(12));
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
