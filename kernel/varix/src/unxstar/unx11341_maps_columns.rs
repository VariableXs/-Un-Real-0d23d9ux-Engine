//! UNX-F11341 /proc/self/maps 格式逐列真值（0xeb50 段）
//!
//! `/proc/self/maps` 一行只有六列，但每一列都有自己的**格式真值**，错一个
//! 字节就会让用户态解析器（glibc 的 `dl_iterate_phdr`、各类崩溃回溯器）
//! 读出垃圾：
//!
//! ```text
//! 400000-401000 r--p 00002000 00:08 1234 /init
//! ^起止    ^权限 ^文件偏移 ^设备 ^节点 ^路径
//! ```
//!
//! - 起止是**小写十六进制、无 0x、无补零**，中间一个 `-`；
//! - 权限恒 4 字节，`r`/`w`/`x` 缺位写 `-`，第 4 位是 `p`（私有）或 `s`
//!   （共享），不可能两者并存，也不可能都不写；
//! - 偏移恒 **8 位小写十六进制、左补零**——解析器按定宽切列，短一位就错位；
//! - 设备是 `主:次`，各 **2 位**十六进制；
//! - 节点是十进制，匿名映射写 0；
//! - 路径是可选列，只有存在时才追加一个空格和路径，行尾不带空格。
//!
//! 本模块逐列建模，每列有自己的校验和错误码，渲染时逐字节写入定容缓冲，
//! 再按字节比对真值。这样「格式对不对」不靠肉眼，而是一条可复算的字节串。
//!
//! 闸门次序（逐列设置）：区间先判「起止倒置」再判「页对齐」；权限先判
//! 「私有/共享互斥」再判「至少一个访问位」；偏移判页对齐；设备判主次上限；
//! 路径判长度上限。渲染要求前五列齐备，缺列返回 `MC_EWARM`。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xeb50;

/// 起止倒置或为零长。
pub const MC_ESPAN: u32 = SEG_BASE + 0x01;
/// 起止未页对齐。
pub const MC_EALIGN: u32 = SEG_BASE + 0x02;
/// 权限无访问位。
pub const MC_EPERM: u32 = SEG_BASE + 0x03;
/// 私有/共享标志冲突或缺失。
pub const MC_ESHARE: u32 = SEG_BASE + 0x04;
/// 偏移未页对齐。
pub const MC_EOFFSET: u32 = SEG_BASE + 0x05;
/// 设备号越界。
pub const MC_EDEV: u32 = SEG_BASE + 0x06;
/// 路径过长。
pub const MC_ENAME: u32 = SEG_BASE + 0x07;
/// 缓冲或下标越界。
pub const MC_ENOBS: u32 = SEG_BASE + 0x08;
/// 列未齐备。
pub const MC_EWARM: u32 = SEG_BASE + 0x09;
/// 账本失衡。
pub const MC_EBAL: u32 = SEG_BASE + 0x0a;
/// 账本槽位乱序。
pub const MC_EORDER: u32 = SEG_BASE + 0x0b;
/// 观测值与复算不符。
pub const MC_EMISMATCH: u32 = SEG_BASE + 0x0c;
/// 冻结后重放不一致。
pub const MC_ESEAL: u32 = SEG_BASE + 0x0d;

/// 行缓冲容量。
pub const LINE_CAP: usize = 160;
/// 路径列容量。
pub const NAME_CAP: usize = 24;
/// 页大小。
pub const PAGE: u64 = 0x1000;
/// 设备主号上限。
pub const DEV_MAJOR_MAX: u32 = 0xfff;
/// 设备次号上限。
pub const DEV_MINOR_MAX: u32 = 0xfffff;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 权限位：读。
pub const P_R: u8 = 1;
/// 权限位：写。
pub const P_W: u8 = 2;
/// 权限位：执行。
pub const P_X: u8 = 4;
/// 权限位：私有。
pub const P_PRIVATE: u8 = 8;
/// 权限位：共享。
pub const P_SHARED: u8 = 16;

/// 操作码：设置区间。
pub const OP_SPAN: u8 = 1;
/// 操作码：设置权限。
pub const OP_PERM: u8 = 2;
/// 操作码：设置偏移。
pub const OP_OFF: u8 = 3;
/// 操作码：设置设备。
pub const OP_DEV: u8 = 4;
/// 操作码：设置节点。
pub const OP_INO: u8 = 5;
/// 操作码：设置路径。
pub const OP_NAME: u8 = 6;
/// 操作码：列数。
pub const OP_COLS: u8 = 7;
/// 操作码：渲染。
pub const OP_RENDER: u8 = 8;

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

/// 一行 maps 记录。
#[derive(Clone, Copy, Debug)]
pub struct Row {
    start: u64,
    end: u64,
    perms: u8,
    off: u64,
    dev_major: u32,
    dev_minor: u32,
    ino: u64,
    name: [u8; NAME_CAP],
    name_len: usize,
    has_span: bool,
    has_perm: bool,
    has_off: bool,
    has_dev: bool,
    has_ino: bool,
    buf: [u8; LINE_CAP],
    buf_len: usize,
}

impl Row {
    /// 空行。
    pub const fn new() -> Row {
        Row {
            start: 0,
            end: 0,
            perms: 0,
            off: 0,
            dev_major: 0,
            dev_minor: 0,
            ino: 0,
            name: [0; NAME_CAP],
            name_len: 0,
            has_span: false,
            has_perm: false,
            has_off: false,
            has_dev: false,
            has_ino: false,
            buf: [0; LINE_CAP],
            buf_len: 0,
        }
    }

    /// 设置区间，返回区间长度。
    pub fn set_span(&mut self, start: u64, end: u64) -> Result<u32, u32> {
        if start >= end {
            return Err(MC_ESPAN);
        }
        if start % PAGE != 0 || end % PAGE != 0 {
            return Err(MC_EALIGN);
        }
        self.start = start;
        self.end = end;
        self.has_span = true;
        Ok((end - start) as u32)
    }

    /// 设置权限，返回权限字节。
    pub fn set_perms(
        &mut self,
        r: bool,
        w: bool,
        x: bool,
        private: bool,
        shared: bool,
    ) -> Result<u32, u32> {
        if private == shared {
            return Err(MC_ESHARE);
        }
        if !r && !w && !x {
            return Err(MC_EPERM);
        }
        let mut b = 0u8;
        if r {
            b |= P_R;
        }
        if w {
            b |= P_W;
        }
        if x {
            b |= P_X;
        }
        b |= if private { P_PRIVATE } else { P_SHARED };
        self.perms = b;
        self.has_perm = true;
        Ok(b as u32)
    }

    /// 设置文件偏移，返回偏移值。
    pub fn set_offset(&mut self, off: u64) -> Result<u32, u32> {
        if off % PAGE != 0 {
            return Err(MC_EOFFSET);
        }
        self.off = off;
        self.has_off = true;
        Ok(off as u32)
    }

    /// 设置设备号，返回编码值（主号 << 20 | 次号）。
    pub fn set_dev(&mut self, major: u32, minor: u32) -> Result<u32, u32> {
        if major > DEV_MAJOR_MAX || minor > DEV_MINOR_MAX {
            return Err(MC_EDEV);
        }
        self.dev_major = major;
        self.dev_minor = minor;
        self.has_dev = true;
        Ok((major << 20) | minor)
    }

    /// 设置节点号，返回节点值。
    pub fn set_inode(&mut self, ino: u64) -> Result<u32, u32> {
        self.ino = ino;
        self.has_ino = true;
        Ok(ino as u32)
    }

    /// 设置路径列，返回路径长度（0 表示匿名映射，渲染时不带路径列）。
    pub fn set_name(&mut self, name: &[u8]) -> Result<usize, u32> {
        if name.len() > NAME_CAP {
            return Err(MC_ENAME);
        }
        let mut i = 0usize;
        while i < name.len() {
            if let (Some(dst), Some(src)) = (self.name.get_mut(i), name.get(i)) {
                *dst = *src;
            }
            i += 1;
        }
        self.name_len = name.len();
        Ok(name.len())
    }

    /// 列数（固定 6）。
    pub fn columns(&self) -> usize {
        6
    }

    /// 路径长度。
    pub fn name_len(&self) -> usize {
        self.name_len
    }

    /// 区间长度。
    pub fn span_len(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    /// 第 i 个渲染字节。
    pub fn byte_at(&self, i: usize) -> Result<u32, u32> {
        if i >= self.buf_len {
            return Err(MC_ENOBS);
        }
        match self.buf.get(i) {
            Some(b) => Ok(*b as u32),
            None => Err(MC_ENOBS),
        }
    }

    /// 渲染缓冲长度。
    pub fn rendered_len(&self) -> usize {
        self.buf_len
    }

    /// 追加一个字节。
    fn put(&mut self, b: u8) {
        if self.buf_len < LINE_CAP {
            if let Some(dst) = self.buf.get_mut(self.buf_len) {
                *dst = b;
            }
            self.buf_len += 1;
        }
    }

    /// 追加一段字节。
    fn put_slice(&mut self, s: &[u8]) {
        let mut i = 0usize;
        while i < s.len() {
            if let Some(b) = s.get(i) {
                self.put(*b);
            }
            i += 1;
        }
    }

    /// 追加小写十六进制（定宽左补零）。
    fn put_hex(&mut self, v: u64, width: usize) {
        const DIGITS: [u8; 16] = *b"0123456789abcdef";
        let mut i = width;
        while i > 0 {
            i -= 1;
            let shift = (i * 4) as u32;
            let nib = ((v >> shift) & 0xf) as usize;
            match DIGITS.get(nib) {
                Some(d) => self.put(*d),
                None => self.put(b'0'),
            }
        }
    }

    /// 追加十进制。
    fn put_dec(&mut self, v: u64) {
        if v == 0 {
            self.put(b'0');
            return;
        }
        let mut digits = [0u8; 20];
        let mut n = 0usize;
        let mut x = v;
        while x > 0 && n < 20 {
            if let Some(dst) = digits.get_mut(n) {
                *dst = b'0' + (x % 10) as u8;
            }
            x /= 10;
            n += 1;
        }
        while n > 0 {
            n -= 1;
            if let Some(d) = digits.get(n) {
                self.put(*d);
            }
        }
    }

    /// 渲染整行，返回字节数。
    ///
    /// 前五列必须齐备；路径列仅在 `name_len > 0` 时追加。行尾绝不留空格，
    /// 因为不少解析器把行尾空格算进路径列。
    pub fn render(&mut self) -> Result<usize, u32> {
        if !self.has_span || !self.has_perm || !self.has_off || !self.has_dev || !self.has_ino {
            return Err(MC_EWARM);
        }
        self.buf_len = 0;
        self.put_hex(self.start, 8);
        self.put(b'-');
        self.put_hex(self.end, 8);
        self.put(b' ');
        let letters = [P_R, P_W, P_X];
        let chars = [b'r', b'w', b'x'];
        let mut i = 0usize;
        while i < 3 {
            let bit = match letters.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let ch = match chars.get(i) {
                Some(v) => *v,
                None => b'-',
            };
            if (self.perms & bit) != 0 {
                self.put(ch);
            } else {
                self.put(b'-');
            }
            i += 1;
        }
        if (self.perms & P_PRIVATE) != 0 {
            self.put(b'p');
        } else {
            self.put(b's');
        }
        self.put(b' ');
        self.put_hex(self.off, 8);
        self.put(b' ');
        self.put_hex(self.dev_major as u64, 2);
        self.put(b':');
        self.put_hex(self.dev_minor as u64, 2);
        self.put(b' ');
        self.put_dec(self.ino);
        if self.name_len > 0 {
            self.put(b' ');
            let mut k = 0usize;
            while k < self.name_len {
                if let Some(b) = self.name.get(k) {
                    self.put(*b);
                }
                k += 1;
            }
        }
        Ok(self.buf_len)
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
            return Err(MC_EORDER);
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
            return Err(MC_EBAL);
        }
        const VALS: [u32; 8] = [4096, 9, 8192, 8, 1234, 5, 6, 48];
        const CODES: [u32; 6] = [
            MC_ESPAN,
            MC_EPERM,
            MC_ESHARE,
            MC_EOFFSET,
            MC_EDEV,
            MC_ENAME,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(MC_EBAL),
            };
            if s.slot as usize != i {
                return Err(MC_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(MC_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(MC_EMISMATCH);
                        }
                    }
                    None => return Err(MC_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(MC_EMISMATCH);
                        }
                    }
                    None => return Err(MC_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(MC_EBAL);
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
            return Err(MC_EBAL);
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
            Err(MC_ESEAL)
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

/// 十四格账本：五列齐备 → 六列 → 渲染真值 → 六类列错误。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut r = Row::new();

    let _ = l.register(word_cell(0, OP_SPAN, r.set_span(0x400000, 0x401000)));
    let _ = l.register(word_cell(1, OP_PERM, r.set_perms(true, false, false, true, false)));
    let _ = l.register(word_cell(2, OP_OFF, r.set_offset(0x2000)));
    let _ = l.register(word_cell(3, OP_DEV, r.set_dev(0, 8)));
    let _ = l.register(word_cell(4, OP_INO, r.set_inode(1234)));
    let _ = l.register(cell(5, OP_NAME, r.set_name(b"/init")));
    let _ = l.register(cell(6, OP_COLS, Ok(r.columns())));
    let _ = l.register(cell(7, OP_RENDER, r.render()));

    let mut a = Row::new();
    let _ = l.register(word_cell(8, OP_SPAN, a.set_span(0x1000, 0x1000)));

    let mut b = Row::new();
    let _ = l.register(word_cell(9, OP_PERM, b.set_perms(false, false, false, true, false)));

    let mut c = Row::new();
    let _ = l.register(word_cell(10, OP_PERM, c.set_perms(true, false, false, true, true)));

    let mut d = Row::new();
    let _ = l.register(word_cell(11, OP_OFF, d.set_offset(0x123)));

    let mut e = Row::new();
    let _ = l.register(word_cell(12, OP_DEV, e.set_dev(0x2000, 0)));

    let mut f = Row::new();
    let _ = l.register(cell(13, OP_NAME, f.set_name(b"/very/long/path/name/0001")));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：整行逐字节真值。
    const EXPECT: &[u8] = b"00400000-00401000 r--p 00002000 00:08 1234 /init";
    let mut r = Row::new();
    let _ = r.set_span(0x400000, 0x401000);
    let _ = r.set_perms(true, false, false, true, false);
    let _ = r.set_offset(0x2000);
    let _ = r.set_dev(0, 8);
    let _ = r.set_inode(1234);
    let _ = r.set_name(b"/init");
    let n = r.render();
    let mut same = n == Ok(EXPECT.len());
    let mut i = 0usize;
    while i < EXPECT.len() {
        let got = r.byte_at(i);
        let want = match EXPECT.get(i) {
            Some(b) => Ok(*b as u32),
            None => Err(MC_ENOBS),
        };
        if got != want {
            same = false;
        }
        i += 1;
    }
    out[1] = same && r.rendered_len() == 48 && r.span_len() == 4096;

    // 二路：无路径列时行尾不留空格。
    let mut q = Row::new();
    let _ = q.set_span(0x7f000000, 0x7f001000);
    let _ = q.set_perms(true, true, false, true, false);
    let _ = q.set_offset(0);
    let _ = q.set_dev(0, 0);
    let _ = q.set_inode(0);
    let n2 = q.render();
    out[2] = n2 == Ok(39)
        && q.byte_at(18) == Ok(b'r' as u32)
        && q.byte_at(21) == Ok(b'p' as u32)
        && q.byte_at(38) == Ok(b'0' as u32)
        && q.byte_at(39) == Err(MC_ENOBS)
        && q.set_name(b"") == Ok(0)
        && q.render() == Ok(39);

    // 三路：逐列闸门。
    let mut g = Row::new();
    let c1 = g.set_span(0x1000, 0x1000) == Err(MC_ESPAN);
    let c2 = g.set_span(0x1001, 0x2000) == Err(MC_EALIGN);
    let c3 = g.set_perms(false, false, false, true, false) == Err(MC_EPERM);
    let c4 = g.set_perms(true, false, false, true, true) == Err(MC_ESHARE);
    let c5 = g.set_perms(true, false, false, false, false) == Err(MC_ESHARE);
    let c6 = g.set_offset(0x10) == Err(MC_EOFFSET);
    let c7 = g.set_dev(0x1000, 0) == Err(MC_EDEV);
    let c8 = g.set_dev(0, 0x100000) == Err(MC_EDEV);
    let c9 = g.set_name(b"/1234567890123456789012345") == Err(MC_ENAME);
    let c10 = g.render() == Err(MC_EWARM);
    let c11 = g.byte_at(0) == Err(MC_ENOBS);
    out[3] = c1 && c2 && c3 && c4 && c5 && c6 && c7 && c8 && c9 && c10 && c11;

    out
}

/// 十三项判据。
pub fn run_unx11341_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11341.maps_columns");
    let l = build_ledger();

    s.add_dyn(
        "MC1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "MC2-区间列与页对齐",
        l.sample(0).map(|x| x.value) == Some(4096),
        "static",
    );

    s.add(
        "MC3-权限列真值",
        l.sample(1).map(|x| x.value) == Some((P_R | P_PRIVATE) as u32),
        "static",
    );

    s.add(
        "MC4-偏移列八位十六进制",
        l.sample(2).map(|x| x.value) == Some(8192),
        "static",
    );

    s.add(
        "MC5-设备列编码",
        l.sample(3).map(|x| x.value) == Some(8),
        "static",
    );

    s.add(
        "MC6-节点列十进制",
        l.sample(4).map(|x| x.value) == Some(1234),
        "static",
    );

    s.add(
        "MC7-路径列长度",
        l.sample(5).map(|x| x.value) == Some(5),
        "static",
    );

    s.add(
        "MC8-六列计数",
        l.sample(6).map(|x| x.value) == Some(6),
        "static",
    );

    s.add(
        "MC9-整行字节数真值",
        l.sample(7).map(|x| x.value) == Some(48),
        "static",
    );

    s.add(
        "MC10-区间零长与倒置",
        l.sample(8).map(|x| x.code) == Some(MC_ESPAN),
        "static",
    );

    s.add(
        "MC11-权限空访问与冲突",
        l.sample(9).map(|x| x.code) == Some(MC_EPERM)
            && l.sample(10).map(|x| x.code) == Some(MC_ESHARE),
        "static",
    );

    s.add(
        "MC12-偏移与设备越界",
        l.sample(11).map(|x| x.code) == Some(MC_EOFFSET)
            && l.sample(12).map(|x| x.code) == Some(MC_EDEV)
            && l.sample(13).map(|x| x.code) == Some(MC_ENAME),
        "static",
    );

    s.add(
        "MC13-冻结重放四注入",
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
        let s = run_unx11341_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn renders_exact_line() {
        let mut r = Row::new();
        let _ = r.set_span(0x400000, 0x401000);
        let _ = r.set_perms(true, false, false, true, false);
        let _ = r.set_offset(0x2000);
        let _ = r.set_dev(0, 8);
        let _ = r.set_inode(1234);
        let _ = r.set_name(b"/init");
        assert_eq!(r.render(), Ok(48));
        assert_eq!(r.byte_at(0), Ok(b'0' as u32));
        assert_eq!(r.byte_at(17), Ok(b' ' as u32));
        assert_eq!(r.byte_at(18), Ok(b'r' as u32));
        assert_eq!(r.byte_at(21), Ok(b'p' as u32));
        assert_eq!(r.byte_at(47), Ok(b't' as u32));
        assert_eq!(r.byte_at(48), Err(MC_ENOBS));
    }

    #[test]
    fn optional_path_column() {
        let mut r = Row::new();
        let _ = r.set_span(0x1000, 0x2000);
        let _ = r.set_perms(false, true, false, false, true);
        let _ = r.set_offset(0);
        let _ = r.set_dev(1, 2);
        let _ = r.set_inode(9);
        assert_eq!(r.render(), Ok(39));
        assert_eq!(r.byte_at(21), Ok(b's' as u32));
        assert_eq!(r.set_name(b"/x"), Ok(2));
        assert_eq!(r.render(), Ok(42));
    }

    #[test]
    fn span_and_perm_gates() {
        let mut r = Row::new();
        assert_eq!(r.set_span(0x2000, 0x1000), Err(MC_ESPAN));
        assert_eq!(r.set_span(0x2000, 0x2000), Err(MC_ESPAN));
        assert_eq!(r.set_span(0x2001, 0x3000), Err(MC_EALIGN));
        assert_eq!(r.set_perms(false, false, false, true, false), Err(MC_EPERM));
        assert_eq!(r.set_perms(true, false, false, true, true), Err(MC_ESHARE));
        assert_eq!(r.set_perms(true, false, false, false, false), Err(MC_ESHARE));
        assert_eq!(r.set_perms(true, false, false, true, false), Ok(9));
    }

    #[test]
    fn offset_and_dev_gates() {
        let mut r = Row::new();
        assert_eq!(r.set_offset(1), Err(MC_EOFFSET));
        assert_eq!(r.set_offset(0x1000), Ok(0x1000));
        assert_eq!(r.set_dev(0x1000, 0), Err(MC_EDEV));
        assert_eq!(r.set_dev(0, 0x100000), Err(MC_EDEV));
        assert_eq!(r.set_dev(0xfff, 0xfffff), Ok(0xffff_ffff));
    }

    #[test]
    fn render_requires_all_columns() {
        let mut r = Row::new();
        assert_eq!(r.render(), Err(MC_EWARM));
        let _ = r.set_span(0x1000, 0x2000);
        assert_eq!(r.render(), Err(MC_EWARM));
    }

    #[test]
    fn name_capacity_is_exact() {
        let mut r = Row::new();
        assert_eq!(r.set_name(b"0123456789abcdefghijklmn"), Ok(NAME_CAP));
        assert_eq!(r.name_len(), NAME_CAP);
        assert_eq!(r.set_name(b"0123456789abcdefghijklmno"), Err(MC_ENAME));
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
