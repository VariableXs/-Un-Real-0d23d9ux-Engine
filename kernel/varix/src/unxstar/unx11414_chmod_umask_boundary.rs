//! UNX-F11414 chmod 错误档与 umask 边界
//!
//! chmod 的参数诊断是分档的：先看字符是否属于本工具的字母表，再看八进制数字
//! 是否越界，再看位数，再看权限位是否超过 `0o7777`，最后才轮到 who 与操作符
//! 的结构。哪一档先失败，报出的就是哪一档——把「位数过多」报成「数字越界」
//! 会让真正的修复方向错位，因此本账把档位次序当成可观测事实。
//!
//! umask 的边界同样是两套语义：
//!
//! 1. **建档**：`create(req)` 必须把 umask 从请求里扣掉，`0o666` 在 `0o027`
//!    下只能落成 `0o640`；
//! 2. **chmod**：已有文件的 `chmod(req)` 完全无视 umask，`0o666` 就是 `0o666`；
//! 3. **符号模式**：显式写 who（`a+x`）无视 umask；省略 who（`+x`）则把 umask
//!    当作掩蔽网，umask 挡住的位不置位、也不清除。
//!
//! 闸门次序：
//! - `Policy::new`：umask 上界；
//! - `octal`：空串/未知字符 → 数字越界 → 位数 → 权限位与特殊位开关；
//! - `symbolic`：空串 → who 结构 → 操作符 → 权限字母（含特殊位开关）→ umask；
//! - `create`/`chmod`：权限位上界 → 特殊位开关 → 落值。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xefe0;

/// 无法识别的字符（含重复的 `X`）。
pub const UM_ECHAR: u32 = SEG_BASE + 0x01;
/// 八进制数字越界（8/9）。
pub const UM_EDIGIT: u32 = SEG_BASE + 0x02;
/// 八进制位数超过四位。
pub const UM_EWIDTH: u32 = SEG_BASE + 0x03;
/// 权限位超出允许范围。
pub const UM_EBITS: u32 = SEG_BASE + 0x04;
/// who 段非法或重复。
pub const UM_EWHO: u32 = SEG_BASE + 0x05;
/// 操作符缺失或非法。
pub const UM_EOP: u32 = SEG_BASE + 0x06;
/// 特殊位未获准。
pub const UM_ESPEC: u32 = SEG_BASE + 0x07;
/// umask 超出九位。
pub const UM_EUMASK: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const UM_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const UM_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const UM_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const UM_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const UM_ENOBS: u32 = SEG_BASE + 0x0d;

/// 子句容量。
pub const CAP: usize = 8;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 权限位总上界。
pub const MODE_MAX: u32 = 0o7777;
/// umask 上界。
pub const UMASK_MAX: u32 = 0o777;
/// 八进制位数上限。
pub const WIDTH_MAX: usize = 4;

/// 操作码：解析八进制。
pub const OP_OCTAL: u8 = 1;
/// 操作码：解析符号模式。
pub const OP_SYM: u8 = 2;
/// 操作码：按建档语义落值。
pub const OP_CREATE: u8 = 3;
/// 操作码：按 chmod 语义落值。
pub const OP_CHMOD: u8 = 4;
/// 操作码：观察 umask。
pub const OP_UMASK: u8 = 5;

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

/// 三档权限按 who 展开到九位。
fn spread(who: u8, bits: u8) -> u16 {
    let b = (bits & 0o7) as u16;
    let mut m = 0u16;
    if who & 1 != 0 {
        m |= b << 6;
    }
    if who & 2 != 0 {
        m |= b << 3;
    }
    if who & 4 != 0 {
        m |= b;
    }
    m
}

/// 三档特殊位按 who 展开。
fn special_mask(who: u8) -> u16 {
    let mut m = 0u16;
    if who & 1 != 0 {
        m |= 0o4000;
    }
    if who & 2 != 0 {
        m |= 0o2000;
    }
    if who & 4 != 0 {
        m |= 0o1000;
    }
    m
}

/// 一档参数策略：umask 与特殊位开关。
#[derive(Clone, Copy, Debug)]
pub struct Policy {
    umask: u16,
    special: bool,
}

impl Policy {
    /// 开户；umask 超过 `0o777` 归 `EUMASK`。
    pub fn new(umask: u32, special: bool) -> Result<Policy, u32> {
        if umask > UMASK_MAX {
            return Err(UM_EUMASK);
        }
        Ok(Policy { umask: umask as u16, special })
    }

    /// umask 真值。
    pub fn umask(&self) -> u32 {
        self.umask as u32
    }

    /// 特殊位是否获准。
    pub fn special(&self) -> bool {
        self.special
    }

    /// 解析八进制模式串。
    ///
    /// 闸门次序：空串/未知字符 → 数字越界 → 位数 → 权限位上界 → 特殊位开关。
    /// 前导零也计入位数，所以 `000755` 是六位而不是三位。
    pub fn octal(&self, src: &[u8]) -> Result<u32, u32> {
        if src.is_empty() {
            return Err(UM_ECHAR);
        }
        let mut i = 0usize;
        while i < src.len() {
            let ch = match src.get(i) {
                Some(v) => *v,
                None => return Err(UM_ECHAR),
            };
            if ch < b'0' || ch > b'7' {
                if ch == b'8' || ch == b'9' {
                    return Err(UM_EDIGIT);
                }
                return Err(UM_ECHAR);
            }
            i = i.saturating_add(1);
        }
        if src.len() > WIDTH_MAX {
            return Err(UM_EWIDTH);
        }
        let mut v = 0u32;
        let mut k = 0usize;
        while k < src.len() {
            let ch = match src.get(k) {
                Some(x) => *x,
                None => return Err(UM_ECHAR),
            };
            v = v.saturating_mul(8).saturating_add((ch - b'0') as u32);
            k = k.saturating_add(1);
        }
        if v > MODE_MAX {
            return Err(UM_EBITS);
        }
        if !self.special && v > 0o777 {
            return Err(UM_EBITS);
        }
        Ok(v)
    }

    /// 解析符号模式并作用在 `cur` 上。
    ///
    /// 闸门次序：空串 → who 字符 → 操作符 → 权限字母 → umask 掩蔽。
    /// `who` 省略时以 umask 为掩蔽网；显式 `who` 时 umask 完全不参与。
    pub fn symbolic(&self, src: &[u8], cur: u32) -> Result<u32, u32> {
        if src.is_empty() {
            return Err(UM_ECHAR);
        }
        let mut mode = cur & MODE_MAX;
        let um = (self.umask & 0o777) as u16;
        let mut i = 0usize;
        while i < src.len() {
            let mut who = 0u8;
            let mut explicit = false;
            while i < src.len() {
                let ch = match src.get(i) {
                    Some(v) => *v,
                    None => return Err(UM_ECHAR),
                };
                if ch == b'+' || ch == b'-' || ch == b'=' {
                    break;
                }
                let bit = match ch {
                    b'u' => 1u8,
                    b'g' => 2u8,
                    b'o' => 4u8,
                    b'a' => 7u8,
                    _ => return Err(UM_ECHAR),
                };
                if who & bit != 0 {
                    return Err(UM_EWHO);
                }
                who |= bit;
                explicit = true;
                i = i.saturating_add(1);
            }
            if i >= src.len() {
                return Err(UM_EOP);
            }
            let op = match src.get(i) {
                Some(v) => *v,
                None => return Err(UM_EOP),
            };
            if op != b'+' && op != b'-' && op != b'=' {
                return Err(UM_EOP);
            }
            i = i.saturating_add(1);
            let mut bits = 0u8;
            let mut xcap = false;
            let mut sp = 0u16;
            let mut k = 0u8;
            while i < src.len() {
                let ch = match src.get(i) {
                    Some(v) => *v,
                    None => return Err(UM_ECHAR),
                };
                if ch == b',' {
                    break;
                }
                match ch {
                    b'r' => bits |= 4,
                    b'w' => bits |= 2,
                    b'x' => bits |= 1,
                    b'X' => {
                        if xcap {
                            return Err(UM_ECHAR);
                        }
                        xcap = true;
                    }
                    b's' => {
                        if !self.special {
                            return Err(UM_ESPEC);
                        }
                        if who == 0 {
                            sp |= 0o6000;
                        } else {
                            if who & 1 != 0 {
                                sp |= 0o4000;
                            }
                            if who & 2 != 0 {
                                sp |= 0o2000;
                            }
                        }
                    }
                    b't' => {
                        if !self.special {
                            return Err(UM_ESPEC);
                        }
                        if who == 0 || who & 4 != 0 {
                            sp |= 0o1000;
                        }
                    }
                    _ => return Err(UM_ECHAR),
                }
                k = k.saturating_add(1);
                i = i.saturating_add(1);
            }
            if k == 0 && op != b'=' {
                return Err(UM_ECHAR);
            }
            let w = if who == 0 { 7u8 } else { who };
            let mut mask = spread(w, bits);
            if xcap && (mode & 0o111) != 0 {
                mask |= spread(w, 1);
            }
            let mut clear = spread(w, 7);
            if !explicit {
                mask &= !um;
                clear &= !um;
            }
            let set = (mask as u32) | (sp as u32);
            mode = match op {
                b'+' => (mode | set) & MODE_MAX,
                b'-' => (mode & !set) & MODE_MAX,
                b'=' => ((mode & !((clear as u32) | (special_mask(w) as u32))) | set) & MODE_MAX,
                _ => return Err(UM_EOP),
            };
            if i < src.len() && src.get(i) == Some(&b',') {
                i = i.saturating_add(1);
                if i == src.len() {
                    return Err(UM_ECHAR);
                }
            }
        }
        Ok(mode)
    }

    /// 值域闸门：权限位上界 → 特殊位开关。
    fn gate(&self, req: u32) -> Result<u32, u32> {
        if req > MODE_MAX {
            return Err(UM_EBITS);
        }
        if !self.special && req > 0o777 {
            return Err(UM_EBITS);
        }
        Ok(req)
    }

    /// 建档落值：请求位扣掉 umask。
    pub fn create(&self, req: u32) -> Result<u32, u32> {
        let v = self.gate(req)?;
        Ok(v & !(self.umask as u32))
    }

    /// chmod 落值：忽略 umask，并报告是否产生变化。
    pub fn chmod(&self, req: u32, cur: u32) -> Result<(u32, bool), u32> {
        let v = self.gate(req)?;
        Ok((v, v != (cur & MODE_MAX)))
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

/// `Result<u8, u32>` → 样本。
fn byte_cell(slot: u8, op: u8, r: Result<u8, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
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
            return Err(UM_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len = self.len.saturating_add(1);
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
            if let Some(v) = self.rows.get(i) {
                if v.ok {
                    n = n.saturating_add(1);
                }
            }
            i = i.saturating_add(1);
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len.saturating_sub(self.ok_count())
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(UM_EBAL);
        }
        const VALS: [u32; 8] = [493, 420, 2541, 492, 493, 416, 384, 292];
        const CODES: [u32; 6] =
            [UM_ECHAR, UM_EDIGIT, UM_EWIDTH, UM_EWHO, UM_EOP, UM_EUMASK];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let v = match self.rows.get(i) {
                Some(x) => *x,
                None => return Err(UM_EBAL),
            };
            if v.slot as usize != i {
                return Err(UM_ESLOT);
            }
            if v.ok {
                if v.code != 0 {
                    return Err(UM_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if v.value != *w {
                            return Err(UM_EVALUE);
                        }
                    }
                    None => return Err(UM_EBAL),
                }
                vi = vi.saturating_add(1);
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if v.code != *w {
                            return Err(UM_EVALUE);
                        }
                    }
                    None => return Err(UM_EBAL),
                }
                ci = ci.saturating_add(1);
            }
            i = i.saturating_add(1);
        }
        if vi != 8 || ci != 6 {
            return Err(UM_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.rows.get(i) {
                h = fold(h, v.slot as u64);
                h = fold(h, v.op as u64);
                h = fold(h, v.ok as u64);
                h = fold(h, v.code as u64);
                h = fold(h, v.value as u64);
            }
            i = i.saturating_add(1);
        }
        fold(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(UM_EBAL);
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
            Err(UM_ESEAL)
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

/// 十四格账本：八档正解 → 六档错误。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let p = match Policy::new(0o027, true) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let plain = match Policy::new(0, false) {
        Ok(v) => v,
        Err(_) => return l,
    };

    let _ = l.register(word_cell(0, OP_OCTAL, p.octal(b"755")));
    let _ = l.register(word_cell(1, OP_OCTAL, p.octal(b"644")));
    let _ = l.register(word_cell(2, OP_OCTAL, p.octal(b"4755")));
    let _ = l.register(word_cell(3, OP_SYM, p.symbolic(b"+x", 0o644)));
    let _ = l.register(word_cell(4, OP_SYM, p.symbolic(b"a+x", 0o644)));
    let _ = l.register(word_cell(5, OP_CREATE, p.create(0o666)));
    let _ = l.register(word_cell(6, OP_CHMOD, p.chmod(0o600, 0o640).map(|(m, _)| m)));
    let _ = l.register(word_cell(7, OP_SYM, p.symbolic(b"a=r", 0o777)));

    let _ = l.register(word_cell(8, OP_OCTAL, plain.octal(b"77x")));
    let _ = l.register(word_cell(9, OP_OCTAL, plain.octal(b"779")));
    let _ = l.register(word_cell(10, OP_OCTAL, plain.octal(b"12345")));
    let _ = l.register(word_cell(11, OP_SYM, plain.symbolic(b"uu+x", 0o644)));
    let _ = l.register(word_cell(12, OP_SYM, plain.symbolic(b"u", 0o644)));
    let _ = l.register(word_cell(13, OP_UMASK, Policy::new(0o1000, true).map(|v| v.umask())));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：省略 who 受 umask 掩蔽，显式 who 越网，建档扣 umask。
    let p = match Policy::new(0o077, true) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let implicit = p.symbolic(b"+x", 0o644);
    let explicit = p.symbolic(b"a+x", 0o644);
    let chmod = p.chmod(0o666, 0o644);
    let mk1 = p.create(0o666);
    let mk2 = p.create(0o777);
    out[1] = implicit == Ok(0o744)
        && explicit == Ok(0o755)
        && chmod == Ok((0o666, true))
        && mk1 == Ok(0o600)
        && mk2 == Ok(0o700)
        && p.umask() == 0o077;

    // 二路：八档错误各自可辨。
    let bad_umask = matches!(Policy::new(0o1000, true), Err(UM_EUMASK));
    let plain = match Policy::new(0, false) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let special_p = match Policy::new(0, true) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let e_char = plain.octal(b"") == Err(UM_ECHAR);
    let e_digit = plain.octal(b"9") == Err(UM_EDIGIT);
    let e_width = plain.octal(b"12345") == Err(UM_EWIDTH);
    let e_bits = plain.octal(b"4755") == Err(UM_EBITS);
    let e_spec = plain.symbolic(b"u+s", 0o755) == Err(UM_ESPEC);
    let e_spec_t = plain.symbolic(b"a+t", 0o755) == Err(UM_ESPEC);
    let e_who = plain.symbolic(b"uu+x", 0o644) == Err(UM_EWHO);
    let e_op = plain.symbolic(b"u", 0o644) == Err(UM_EOP);
    let e_char2 = plain.symbolic(b"u+?", 0o644) == Err(UM_ECHAR);
    let sp_ok = special_p.symbolic(b"u+s", 0o755) == Ok(0o4755);
    let sp_ok_t = special_p.symbolic(b"a+t", 0o755) == Ok(0o1755);
    out[2] = bad_umask
        && e_char
        && e_digit
        && e_width
        && e_bits
        && e_spec
        && e_spec_t
        && e_who
        && e_op
        && e_char2
        && sp_ok
        && sp_ok_t;

    // 三路：显式 who 的赋值、等号清位、X 与特殊位。
    let q = match Policy::new(0o027, true) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let s1 = q.symbolic(b"u=rw,go=r", 0o777);
    let s2 = q.symbolic(b"a+X", 0o644);
    let s3 = q.symbolic(b"a+X", 0o610);
    let s4 = q.symbolic(b"=rw", 0o777);
    let s5 = q.symbolic(b"o-x", 0o755);
    let s6 = q.symbolic(b"u+s", 0o755);
    out[3] = s1 == Ok(0o644)
        && s2 == Ok(0o644)
        && s3 == Ok(0o711)
        && s4 == Ok(0o667)
        && s5 == Ok(0o754)
        && s6 == Ok(0o4755)
        && q.create(0o600) == Ok(0o600)
        && q.chmod(0o600, 0o600) == Ok((0o600, false));

    out
}

/// 十三项判据。
pub fn run_unx11414_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11414.chmod_umask_boundary");
    let l = build_ledger();

    s.add_dyn(
        "UM1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "UM2-八进制正解三档",
        l.sample(0).map(|x| x.value) == Some(493)
            && l.sample(1).map(|x| x.value) == Some(420)
            && l.sample(2).map(|x| x.value) == Some(2541),
        "static",
    );

    s.add(
        "UM3-数字与宽度分档",
        {
            let p = match Policy::new(0, true) {
                Ok(v) => v,
                Err(_) => return s,
            };
            p.octal(b"88888") == Err(UM_EDIGIT)
                && p.octal(b"12345") == Err(UM_EWIDTH)
                && p.octal(b"0755") == Ok(0o755)
                && l.sample(9).map(|x| x.code) == Some(UM_EDIGIT)
                && l.sample(10).map(|x| x.code) == Some(UM_EWIDTH)
        },
        "static",
    );

    s.add(
        "UM4-省略who受umask",
        {
            let p = match Policy::new(0o077, true) {
                Ok(v) => v,
                Err(_) => return s,
            };
            p.symbolic(b"+x", 0o644) == Ok(0o744)
                && p.symbolic(b"+w", 0o644) == Ok(0o644)
                && p.symbolic(b"-w", 0o666) == Ok(0o466)
                && l.sample(3).map(|x| x.value) == Some(492)
        },
        "static",
    );

    s.add(
        "UM5-显式who越umask",
        {
            let p = match Policy::new(0o077, true) {
                Ok(v) => v,
                Err(_) => return s,
            };
            p.symbolic(b"a+x", 0o644) == Ok(0o755)
                && p.symbolic(b"go+w", 0o644) == Ok(0o666)
                && p.symbolic(b"o-rwx", 0o777) == Ok(0o770)
                && l.sample(4).map(|x| x.value) == Some(493)
        },
        "static",
    );

    s.add(
        "UM6-建档扣umask",
        {
            let p = match Policy::new(0o077, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            p.create(0o666) == Ok(0o600)
                && p.create(0o777) == Ok(0o700)
                && p.create(0o4755) == Err(UM_EBITS)
                && l.sample(5).map(|x| x.value) == Some(416)
        },
        "static",
    );

    s.add(
        "UM7-chmod无视umask",
        {
            let p = match Policy::new(0o077, true) {
                Ok(v) => v,
                Err(_) => return s,
            };
            p.chmod(0o666, 0o644) == Ok((0o666, true))
                && p.chmod(0o644, 0o644) == Ok((0o644, false))
                && p.chmod(0o1000, 0o644) == Ok((0o1000, true))
                && l.sample(6).map(|x| x.value) == Some(384)
        },
        "static",
    );

    s.add(
        "UM8-特殊位开关闸门",
        {
            let plain = match Policy::new(0, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let sp = match Policy::new(0, true) {
                Ok(v) => v,
                Err(_) => return s,
            };
            plain.octal(b"4755") == Err(UM_EBITS)
                && plain.symbolic(b"u+s", 0o755) == Err(UM_ESPEC)
                && plain.symbolic(b"g+s", 0o755) == Err(UM_ESPEC)
                && plain.symbolic(b"o+t", 0o755) == Err(UM_ESPEC)
                && sp.octal(b"4755") == Ok(0o4755)
                && sp.symbolic(b"g+s", 0o755) == Ok(0o2755)
                && sp.symbolic(b"a+t", 0o755) == Ok(0o1755)
                && l.sample(2).map(|x| x.value) == Some(2541)
        },
        "static",
    );

    s.add(
        "UM9-who重复与操作符缺失",
        {
            let p = match Policy::new(0, true) {
                Ok(v) => v,
                Err(_) => return s,
            };
            p.symbolic(b"uu+x", 0o644) == Err(UM_EWHO)
                && p.symbolic(b"u", 0o644) == Err(UM_EOP)
                && p.symbolic(b"u?x", 0o644) == Err(UM_ECHAR)
                && p.symbolic(b"z+x", 0o644) == Err(UM_ECHAR)
                && l.sample(11).map(|x| x.code) == Some(UM_EWHO)
                && l.sample(12).map(|x| x.code) == Some(UM_EOP)
        },
        "static",
    );

    s.add(
        "UM10-umask上界与清零",
        {
            let zero = match Policy::new(0o777, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            Policy::new(0o777, false).is_ok()
                && matches!(Policy::new(0o1000, false), Err(UM_EUMASK))
                && zero.create(0o666) == Ok(0)
                && zero.umask() == 0o777
                && l.sample(13).map(|x| x.code) == Some(UM_EUMASK)
        },
        "static",
    );

    s.add(
        "UM11-等号清位与X合流",
        {
            let p = match Policy::new(0o027, true) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let a = p.symbolic(b"a=rw", 0o777);
            let b = p.symbolic(b"=rw", 0o777);
            let c = p.symbolic(b"u=rwx,g=rx,o=", 0o777);
            let d = p.symbolic(b"a+X", 0o600);
            let e = p.symbolic(b"a+X", 0o601);
            a == Ok(0o666)
                && b == Ok(0o667)
                && c == Ok(0o750)
                && d == Ok(0o600)
                && e == Ok(0o711)
        },
        "static",
    );

    s.add(
        "UM12-错误档次序",
        {
            let p = match Policy::new(0, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            p.octal(b"99999") == Err(UM_EDIGIT)
                && p.octal(b"12345") == Err(UM_EWIDTH)
                && p.octal(b"7777") == Err(UM_EBITS)
                && p.symbolic(b"uu+s", 0o755) == Err(UM_EWHO)
                && p.symbolic(b"u+s+x", 0o755) == Err(UM_ESPEC)
                && p.symbolic(b"u+x,", 0o755) == Err(UM_ECHAR)
        },
        "static",
    );

    s.add(
        "UM13-冻结重放四注入",
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

    fn pol(umask: u32, special: bool) -> Policy {
        match Policy::new(umask, special) {
            Ok(v) => v,
            Err(_) => panic!("policy"),
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
        assert_ne!(l.seal(), 0);
        assert_eq!(l.replay(), Ok(l.seal()));
        let mut i = 0usize;
        while i < l.len() {
            assert_eq!(l.sample(i).map(|x| x.slot as usize), Some(i));
            i += 1;
        }
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11414_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn octal_tiers() {
        let p = pol(0, false);
        assert_eq!(p.octal(b"755"), Ok(0o755));
        assert_eq!(p.octal(b"0"), Ok(0));
        assert_eq!(p.octal(b"779"), Err(UM_EDIGIT));
        assert_eq!(p.octal(b"12345"), Err(UM_EWIDTH));
        assert_eq!(p.octal(b"4755"), Err(UM_EBITS));
        assert_eq!(p.octal(b"77x"), Err(UM_ECHAR));
        assert_eq!(p.octal(b""), Err(UM_ECHAR));
        assert_eq!(pol(0, true).octal(b"4755"), Ok(0o4755));
    }

    #[test]
    fn umask_boundary() {
        let p = pol(0o077, true);
        assert_eq!(p.symbolic(b"+x", 0o644), Ok(0o744));
        assert_eq!(p.symbolic(b"a+x", 0o644), Ok(0o755));
        assert_eq!(p.create(0o666), Ok(0o600));
        assert_eq!(p.create(0o777), Ok(0o700));
        assert_eq!(pol(0o777, false).create(0o666), Ok(0));
    }

    #[test]
    fn chmod_ignores_umask() {
        let p = pol(0o077, true);
        assert_eq!(p.chmod(0o666, 0o644), Ok((0o666, true)));
        assert_eq!(p.chmod(0o644, 0o644), Ok((0o644, false)));
        assert_eq!(p.chmod(0o10000, 0), Err(UM_EBITS));
    }

    #[test]
    fn special_bits() {
        let plain = pol(0, false);
        let sp = pol(0, true);
        assert_eq!(plain.symbolic(b"u+s", 0o755), Err(UM_ESPEC));
        assert_eq!(sp.symbolic(b"u+s", 0o755), Ok(0o4755));
        assert_eq!(sp.symbolic(b"g+s", 0o755), Ok(0o2755));
        assert_eq!(sp.symbolic(b"a+t", 0o755), Ok(0o1755));
        assert_eq!(sp.symbolic(b"a=rwxt", 0o777), Ok(0o1777));
    }

    #[test]
    fn symbolic_equals_and_x() {
        let p = pol(0o027, true);
        assert_eq!(p.symbolic(b"u=rw,go=r", 0o777), Ok(0o644));
        assert_eq!(p.symbolic(b"=rw", 0o777), Ok(0o667));
        assert_eq!(p.symbolic(b"a+X", 0o600), Ok(0o600));
        assert_eq!(p.symbolic(b"a+X", 0o601), Ok(0o711));
        assert_eq!(p.symbolic(b"o-x", 0o755), Ok(0o754));
        assert_eq!(p.symbolic(b"uu+x", 0o644), Err(UM_EWHO));
        assert_eq!(p.symbolic(b"u", 0o644), Err(UM_EOP));
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
