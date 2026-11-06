//! UNX-F11574 printf %s 逐位字节透明判据
//!
//! `printf "%s"` 在 C locale 下是**字节透明**的：除 0x00（C 字符串终止符）
//! 之外，0x01..=0xFF 的每一个字节值都必须原样进入输出，不做大小写折叠、
//! 不做宽字符转码、不做非法序列替换。这条判据常被「顺手做了 UTF-8 校验」
//! 的实现破坏——0x80..=0xFF 的孤立字节被替换成 U+FFFD 或直接丢弃，于是
//! `printf %s` 不再是管道里的恒等变换。本模块把判据做成可逐值复算的模型：
//!
//! * 规格层：[`Spec`] 解析 `%[flags][width][.precision][length]conv`，
//!   标志位 `- + space 0 #` 各有冲突规则（`+` 与空格同时出现即拒绝，
//!   `#` 对 `%s` 无定义亦拒绝），宽度/精度各有独立上界。
//! * 变换层：[`format_s`] 只做三件事——在首个 0x00 处截断、按精度截断、
//!   按宽度填充；填充只用空格，绝不触碰参数字节。`%c` 同理只取首字节。
//! * 遍历层：[`walk`] 走整条格式串，字面字节（含 0x80..=0xFF 与 `%`）
//!   原样吐出，`%%` 归一为 `%`，`%s`/`%c` 逐个耗用参数；参数不足或
//!   多余都由 [`PF_EARG`] 显性记账，不允许静默忽略。
//! * 审计层：[`transparent_mask`] 对 0x80..=0xFF 的 128 个值逐一格式化，
//!   逐值判定「输出里存在该字节且长度等于预期」，得到 128 位透明表。
//!
//! 闸门次序：字节 → 标志 → 宽度 → 精度 → 长度 → 转换 → 参数 → 容量 → 渲染。
//!
//! 全模块无 std、无 unsafe、无 panic：缓冲定容 256 字节，越界一律
//! `get()`/`get_mut()`，加法一律 `saturating_*`；失败全部落 13 个错误码。
//! 账本 14 格（8 成 6 败）在 [`Ledger::audit`] 逐格对账，冻结后重放同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf910;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 输出缓冲容量。
pub const BUF_CAP: usize = 256;

/// 宽度上界。
pub const WIDTH_MAX: u16 = 128;

/// 精度上界。
pub const PREC_MAX: u16 = 64;

/// 参数个数上界。
pub const ARG_CAP: usize = 8;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 参数或格式含非法字节（嵌入 0x00）。
pub const PF_EBYTE: u32 = SEG_BASE + 0x01;
/// 规格结构错误（悬空 `%`、`*` 宽度）。
pub const PF_ESPEC: u32 = SEG_BASE + 0x02;
/// 转换符或长度修饰不被 `%s` 面支持。
pub const PF_ECONV: u32 = SEG_BASE + 0x03;
/// 宽度超上界。
pub const PF_EWIDE: u32 = SEG_BASE + 0x04;
/// 精度超上界。
pub const PF_EPREC: u32 = SEG_BASE + 0x05;
/// 参数不足或多余。
pub const PF_EARG: u32 = SEG_BASE + 0x06;
/// 标志冲突（`+` 与空格、`#` 对 `%s`）。
pub const PF_EFLAG: u32 = SEG_BASE + 0x07;
/// 输出超缓冲容量。
pub const PF_EOVER: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const PF_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const PF_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const PF_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const PF_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const PF_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 规格解析。
pub const OP_SPEC: u8 = 0x01;
/// 单参数格式化。
pub const OP_FMT: u8 = 0x02;
/// 填充。
pub const OP_PAD: u8 = 0x03;
/// 透明位图。
pub const OP_TRANS: u8 = 0x04;
/// 零字节截断。
pub const OP_NUL: u8 = 0x05;
/// 计数。
pub const OP_COUNT: u8 = 0x06;
/// 整串遍历。
pub const OP_WALK: u8 = 0x07;

// ---------------------------------------------------------------------------
// 哈希
// ---------------------------------------------------------------------------

/// FNV 偏移基。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 素数。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个字节（逐位八次乘法）。
fn fold_byte(h: u64, b: u8) -> u64 {
    let mut v = h ^ (b as u64);
    let mut k = 0usize;
    while k < 8 {
        v = v.wrapping_mul(FNV_PRIME);
        k += 1;
    }
    v
}

/// 折入一个字。
fn fold(h: u64, w: u64) -> u64 {
    let mut v = h;
    let mut sh = 0u32;
    while sh < 64 {
        let b = ((w >> sh) & 0xff) as u8;
        v = fold_byte(v, b);
        sh += 8;
    }
    v
}

// ---------------------------------------------------------------------------
// 定容缓冲
// ---------------------------------------------------------------------------

/// 定容输出缓冲。
#[derive(Clone, Copy)]
pub struct Buf {
    bytes: [u8; BUF_CAP],
    len: usize,
}

impl Buf {
    /// 空缓冲。
    pub const fn new() -> Buf {
        Buf { bytes: [0u8; BUF_CAP], len: 0 }
    }

    /// 已写长度。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 剩余容量。
    pub fn spare(&self) -> usize {
        BUF_CAP.saturating_sub(self.len)
    }

    /// 推入一个字节。
    pub fn push(&mut self, b: u8) -> Result<(), u32> {
        if self.len >= BUF_CAP {
            return Err(PF_EOVER);
        }
        match self.bytes.get_mut(self.len) {
            Some(dst) => {
                *dst = b;
                self.len += 1;
                Ok(())
            }
            None => Err(PF_EOVER),
        }
    }

    /// 推入一段字节。
    pub fn push_bytes(&mut self, src: &[u8]) -> Result<(), u32> {
        let mut i = 0usize;
        while i < src.len() {
            match src.get(i) {
                Some(b) => self.push(*b)?,
                None => return Err(PF_EOVER),
            }
            i += 1;
        }
        Ok(())
    }

    /// 填充 n 个相同字节。
    pub fn pad(&mut self, n: usize, b: u8) -> Result<(), u32> {
        let mut i = 0usize;
        while i < n {
            self.push(b)?;
            i += 1;
        }
        Ok(())
    }

    /// 取字节。
    pub fn byte(&self, i: usize) -> Option<u8> {
        if i >= self.len {
            return None;
        }
        self.bytes.get(i).copied()
    }

    /// 字节切片。
    pub fn as_slice(&self) -> &[u8] {
        self.bytes.get(0..self.len).unwrap_or(&[])
    }

    /// 是否含某字节。
    pub fn contains(&self, b: u8) -> bool {
        let mut i = 0usize;
        while i < self.len {
            if let Some(c) = self.bytes.get(i) {
                if *c == b {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 某字节出现次数。
    pub fn count(&self, b: u8) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(c) = self.bytes.get(i) {
                if *c == b {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 逐字节相等。
    pub fn equals(&self, other: &[u8]) -> bool {
        if self.len != other.len() {
            return false;
        }
        let mut i = 0usize;
        while i < self.len {
            match (self.bytes.get(i), other.get(i)) {
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

    /// 转文本（字节 → char 一一映射，不做 UTF-8 解码）。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(b) = self.bytes.get(i) {
                s.push(*b as char);
            }
            i += 1;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 规格
// ---------------------------------------------------------------------------

/// `%` 规格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spec {
    /// `-` 左对齐。
    pub minus: bool,
    /// `+` 强制符号（对 `%s` 无意义，仅记录）。
    pub plus: bool,
    /// 空格标志。
    pub space: bool,
    /// `0` 零填充（对 `%s` 退化为空格）。
    pub zero: bool,
    /// `#` 备用形式（对 `%s` 非法）。
    pub hash: bool,
    /// 宽度。
    pub width: u16,
    /// 精度。
    pub prec: u16,
    /// 是否给了精度。
    pub has_prec: bool,
    /// 转换符。
    pub conv: u8,
}

impl Spec {
    /// 空白规格。
    pub const fn blank() -> Spec {
        Spec {
            minus: false,
            plus: false,
            space: false,
            zero: false,
            hash: false,
            width: 0,
            prec: 0,
            has_prec: false,
            conv: 0,
        }
    }

    /// 有效填充字节：`%s` 永远用空格（零填充对字符串无定义）。
    pub fn fill(&self) -> u8 {
        b' '
    }

    /// 是否左对齐。
    pub fn left(&self) -> bool {
        self.minus
    }
}

/// 解析 `%` 之后的规格文本，返回规格与耗用字节数。
pub fn parse_spec(s: &[u8]) -> Result<(Spec, usize), u32> {
    let mut sp = Spec::blank();
    let mut i = 0usize;
    loop {
        let c = match s.get(i) {
            Some(v) => *v,
            None => break,
        };
        match c {
            b'-' => {
                if sp.minus {
                    return Err(PF_EFLAG);
                }
                sp.minus = true;
            }
            b'+' => {
                if sp.plus {
                    return Err(PF_EFLAG);
                }
                sp.plus = true;
            }
            b' ' => {
                if sp.space {
                    return Err(PF_EFLAG);
                }
                sp.space = true;
            }
            b'0' => {
                if sp.zero {
                    return Err(PF_EFLAG);
                }
                sp.zero = true;
            }
            b'#' => {
                if sp.hash {
                    return Err(PF_EFLAG);
                }
                sp.hash = true;
            }
            _ => break,
        }
        i += 1;
    }
    if sp.plus && sp.space {
        return Err(PF_EFLAG);
    }
    if sp.zero && sp.minus {
        // 与 C 一致：左对齐时零填充失效，不报错，只退化。
        sp.zero = false;
    }
    let mut wd: u32 = 0;
    let mut wdigits = 0usize;
    while let Some(c) = s.get(i) {
        if *c < b'0' || *c > b'9' {
            break;
        }
        wd = wd.saturating_mul(10).saturating_add((*c - b'0') as u32);
        wdigits += 1;
        i += 1;
        if wd > WIDTH_MAX as u32 {
            return Err(PF_EWIDE);
        }
    }
    if wdigits > 0 {
        sp.width = wd as u16;
    }
    if s.get(i) == Some(&b'*') {
        return Err(PF_ESPEC);
    }
    if s.get(i) == Some(&b'.') {
        i += 1;
        sp.has_prec = true;
        let mut pd: u32 = 0;
        let mut pdigits = 0usize;
        while let Some(c) = s.get(i) {
            if *c < b'0' || *c > b'9' {
                break;
            }
            pd = pd.saturating_mul(10).saturating_add((*c - b'0') as u32);
            pdigits += 1;
            i += 1;
            if pd > PREC_MAX as u32 {
                return Err(PF_EPREC);
            }
        }
        if pdigits == 0 {
            sp.prec = 0;
        } else {
            sp.prec = pd as u16;
        }
        if s.get(i) == Some(&b'*') {
            return Err(PF_ESPEC);
        }
    }
    let mut lm = 0usize;
    while let Some(c) = s.get(i) {
        match *c {
            b'h' | b'l' | b'z' | b'j' | b't' | b'L' => {
                lm += 1;
                i += 1;
                if lm > 2 {
                    return Err(PF_ECONV);
                }
            }
            _ => break,
        }
    }
    let c = match s.get(i) {
        Some(v) => *v,
        None => return Err(PF_ESPEC),
    };
    i += 1;
    if lm > 0 {
        // 长度修饰只对整数转换有意义；`%s`/`%c` 带长度修饰是错配。
        return Err(PF_ECONV);
    }
    match c {
        b's' | b'c' | b'%' => sp.conv = c,
        0 => return Err(PF_EBYTE),
        b'd' | b'i' | b'u' | b'o' | b'x' | b'X' | b'f' | b'e' | b'g' | b'p' => {
            return Err(PF_ECONV)
        }
        _ => return Err(PF_ECONV),
    }
    if sp.hash && c != b'%' {
        return Err(PF_EFLAG);
    }
    if sp.has_prec && c == b'%' {
        return Err(PF_EFLAG);
    }
    Ok((sp, i))
}

/// 规格文本是否合法。
pub fn spec_ok(s: &[u8]) -> bool {
    parse_spec(s).is_ok()
}

// ---------------------------------------------------------------------------
// 格式化
// ---------------------------------------------------------------------------

/// 首个 0x00 的位置。
pub fn first_nul(b: &[u8]) -> Option<usize> {
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c == 0 {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// `%s`：C 语义（止于 0x00）+ 精度 + 宽度。
pub fn format_s(sp: &Spec, arg: &[u8]) -> Result<Buf, u32> {
    let stop = match first_nul(arg) {
        Some(v) => v,
        None => arg.len(),
    };
    let n = if sp.has_prec {
        stop.min(sp.prec as usize)
    } else {
        stop
    };
    let mut out = Buf::new();
    let w = sp.width as usize;
    let padn = w.saturating_sub(n);
    if !sp.left() {
        out.pad(padn, sp.fill())?;
    }
    let mut i = 0usize;
    while i < n {
        match arg.get(i) {
            Some(b) => out.push(*b)?,
            None => return Err(PF_EARG),
        }
        i += 1;
    }
    if sp.left() {
        out.pad(padn, sp.fill())?;
    }
    Ok(out)
}

/// `%s` 严格版：参数内出现 0x00 直接拒绝（审计路径用，不做静默截断）。
pub fn format_s_strict(sp: &Spec, arg: &[u8]) -> Result<Buf, u32> {
    if first_nul(arg).is_some() {
        return Err(PF_EBYTE);
    }
    format_s(sp, arg)
}

/// `%c`：取参数字节串首字节并填充。
pub fn format_c(sp: &Spec, arg: &[u8]) -> Result<Buf, u32> {
    let b = match arg.first() {
        Some(v) => *v,
        None => return Err(PF_EARG),
    };
    let mut out = Buf::new();
    let padn = (sp.width as usize).saturating_sub(1);
    if !sp.left() {
        out.pad(padn, sp.fill())?;
    }
    out.push(b)?;
    if sp.left() {
        out.pad(padn, sp.fill())?;
    }
    Ok(out)
}

/// 单个 `%s` 是否对某字节透明：输出里含该字节，且长度符合预期。
pub fn transparent_at(spec_text: &[u8], b: u8) -> bool {
    let (sp, used) = match parse_spec(spec_text) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if used != spec_text.len() {
        return false;
    }
    if sp.conv != b's' {
        return false;
    }
    let one = [b];
    let out = match format_s(&sp, &one) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let want = if sp.width == 0 { 1 } else { sp.width as usize };
    out.len() == want && out.contains(b)
}

/// 0x80..=0xFF 逐值透明位图。
pub fn transparent_mask(spec_text: &[u8]) -> [bool; 128] {
    let mut mask = [false; 128];
    let mut v = 0x80u32;
    while v <= 0xff {
        if let Some(slot) = mask.get_mut((v - 0x80) as usize) {
            *slot = transparent_at(spec_text, v as u8);
        }
        v += 1;
    }
    mask
}

/// 位图里透明值的个数。
pub fn transparent_count(spec_text: &[u8]) -> u32 {
    let mask = transparent_mask(spec_text);
    let mut n = 0u32;
    let mut i = 0usize;
    while i < mask.len() {
        if let Some(v) = mask.get(i) {
            if *v {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 整串遍历：`%s`/`%c` 耗用参数，`%%` 归一，其余字面照抄。
pub fn walk(fmt: &[u8], args: &[&[u8]]) -> Result<Buf, u32> {
    let mut out = Buf::new();
    let mut ai = 0usize;
    let mut i = 0usize;
    while i < fmt.len() {
        let c = match fmt.get(i) {
            Some(v) => *v,
            None => return Err(PF_EBYTE),
        };
        if c != b'%' {
            if c == 0 {
                return Err(PF_EBYTE);
            }
            out.push(c)?;
            i += 1;
            continue;
        }
        i += 1;
        if fmt.get(i) == Some(&b'%') {
            out.push(b'%')?;
            i += 1;
            continue;
        }
        let rest = fmt.get(i..).unwrap_or(&[]);
        let (sp, used) = parse_spec(rest)?;
        if used == 0 {
            return Err(PF_ESPEC);
        }
        i += used;
        let arg = match args.get(ai) {
            Some(v) => *v,
            None => return Err(PF_EARG),
        };
        match sp.conv {
            b's' => {
                let piece = format_s(&sp, arg)?;
                out.push_bytes(piece.as_slice())?;
            }
            b'c' => {
                let piece = format_c(&sp, arg)?;
                out.push_bytes(piece.as_slice())?;
            }
            _ => return Err(PF_ESPEC),
        }
        ai += 1;
    }
    Ok(out)
}

/// 十六进制转储（诊断用）：每字节两位小写。
pub fn hexdump(b: &[u8]) -> String {
    const HEX: [u8; 16] = *b"0123456789abcdef";
    let mut s = String::new();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(v) = b.get(i) {
            let hi = (v >> 4) & 0xf;
            let lo = v & 0xf;
            if let Some(c) = HEX.get(hi as usize) {
                s.push(*c as char);
            }
            if let Some(c) = HEX.get(lo as usize) {
                s.push(*c as char);
            }
        }
        i += 1;
    }
    s
}

/// 高位字节样本：0x80..=0xBF，共 64 字节。
pub fn high_sample() -> [u8; 64] {
    let mut b = [0u8; 64];
    let mut i = 0usize;
    while i < 64 {
        if let Some(dst) = b.get_mut(i) {
            *dst = 0x80 + i as u8;
        }
        i += 1;
    }
    b
}

// ---------------------------------------------------------------------------
// 样本与账本
// ---------------------------------------------------------------------------

/// 一格样本。
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

/// 账本。
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
            return Err(PF_ESLOT);
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

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取格。
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
            return Err(PF_EBAL);
        }
        const VALS: [u32; 8] = [8, 2, 64, 8, 8, 128, 128, 2];
        const CODES: [u32; 6] = [PF_EWIDE, PF_EPREC, PF_ECONV, PF_ESPEC, PF_EFLAG, PF_EBYTE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(PF_EBAL),
            };
            if s.slot as usize != i {
                return Err(PF_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(PF_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(PF_EVALUE);
                        }
                    }
                    None => return Err(PF_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(PF_EVALUE);
                        }
                    }
                    None => return Err(PF_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(PF_EBAL);
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
            self.rejects = self.rejects.saturating_add(1);
            return Err(PF_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let a = fresh.seal();
        if a == self.seal() {
            Ok(a)
        } else {
            Err(PF_ESEAL)
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

// ---------------------------------------------------------------------------
// 账本组装
// ---------------------------------------------------------------------------

/// `Result<usize, u32>` → 样本。
fn size_cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<u32, u32>` → 样本。
fn u32_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 解析并返回宽度。
fn width_of(s: &[u8]) -> Result<u32, u32> {
    parse_spec(s).map(|(sp, _)| sp.width as u32)
}

/// 解析并返回精度。
fn prec_of(s: &[u8]) -> Result<u32, u32> {
    parse_spec(s).map(|(sp, _)| sp.prec as u32)
}

/// 账本组装：槽 0..7 成、8..13 败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(u32_cell(0, OP_SPEC, width_of(b"8s")));
    let _ = l.register(u32_cell(1, OP_SPEC, prec_of(b".2s")));

    let high = high_sample();
    let plain = match parse_spec(b"s") {
        Ok((sp, _)) => sp,
        Err(c) => return fail_tail(l, c),
    };
    let _ = l.register(size_cell(2, OP_FMT, format_s(&plain, &high).map(|b| b.len())));

    let wide = match parse_spec(b"8s") {
        Ok((sp, _)) => sp,
        Err(c) => return fail_tail(l, c),
    };
    let _ = l.register(size_cell(3, OP_FMT, format_s(&wide, b"abc").map(|b| b.len())));

    let left = match parse_spec(b"-8s") {
        Ok((sp, _)) => sp,
        Err(c) => return fail_tail(l, c),
    };
    let _ = l.register(size_cell(4, OP_FMT, format_s(&left, b"abc").map(|b| b.len())));

    let _ = l.register(u32_cell(5, OP_TRANS, Ok(transparent_count(b"s"))));
    let _ = l.register(u32_cell(6, OP_TRANS, Ok(transparent_count(b".1s"))));

    let _ = l.register(size_cell(
        7,
        OP_NUL,
        format_s(&plain, b"ab\0cd").map(|b| b.len()),
    ));

    let _ = l.register(u32_cell(8, OP_SPEC, width_of(b"200s")));
    let _ = l.register(u32_cell(9, OP_SPEC, prec_of(b".100s")));
    let _ = l.register(u32_cell(10, OP_SPEC, width_of(b"d")));
    let _ = l.register(u32_cell(11, OP_SPEC, width_of(b"")));
    let _ = l.register(u32_cell(12, OP_SPEC, width_of(b"+ s")));
    let _ = l.register(u32_cell(
        13,
        OP_NUL,
        format_s_strict(&plain, b"ab\0cd").map(|_| 0),
    ));

    l
}

/// 构造期失败时补满尾部，保持 14 格形状。
fn fail_tail(mut l: Ledger, code: u32) -> Ledger {
    let mut slot = l.len();
    while slot < SAMPLES {
        let s = Sample { slot: slot as u8, op: OP_WALK, ok: false, code, value: 0 };
        let _ = l.register(s);
        slot += 1;
    }
    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 规格面。
fn spec_ok_face() -> bool {
    let a = match parse_spec(b"8s") {
        Ok((sp, n)) => sp.width == 8 && sp.conv == b's' && n == 2,
        Err(_) => false,
    };
    let b = match parse_spec(b"-12.3s") {
        Ok((sp, n)) => sp.minus && sp.width == 12 && sp.prec == 3 && sp.has_prec && n == 6,
        Err(_) => false,
    };
    let c = match parse_spec(b"c") {
        Ok((sp, _)) => sp.conv == b'c',
        Err(_) => false,
    };
    let d = match parse_spec(b"0-s") {
        Ok((sp, _)) => sp.minus && !sp.zero,
        Err(_) => false,
    };
    let e = match parse_spec(b"%") {
        Ok((sp, _)) => sp.conv == b'%',
        Err(_) => false,
    };
    a && b && c && d && e
}

/// 宽度精度面。
fn width_prec_ok() -> bool {
    let a = parse_spec(b"128s").map(|(sp, _)| sp.width) == Ok(128);
    let b = parse_spec(b"129s").err() == Some(PF_EWIDE);
    let c = parse_spec(b".64s").map(|(sp, _)| sp.prec) == Ok(64);
    let d = parse_spec(b".65s").err() == Some(PF_EPREC);
    let e = parse_spec(b".s").map(|(sp, _)| sp.has_prec && sp.prec == 0) == Ok(true);
    let f = parse_spec(b"*s").err() == Some(PF_ESPEC);
    let g = parse_spec(b".*s").err() == Some(PF_ESPEC);
    a && b && c && d && e && f && g
}

/// 透明面：0x80..=0xFF 在 `%s` / `%.Ns` 下逐值存活。
fn transparency_ok() -> bool {
    let a = transparent_count(b"s") == 128;
    let b = transparent_count(b".1s") == 128;
    let c = transparent_count(b"-1s") == 128;
    let d = transparent_mask(b"s").iter().all(|v| *v);
    let e = transparent_at(b"s", 0x80)
        && transparent_at(b"s", 0xff)
        && transparent_at(b"s", 0xa5);
    let f = transparent_count(b"c") == 0;
    let g = !transparent_at(b"s", 0x00);
    a && b && c && d && e && f && g
}

/// 高位区间面：逐个值等于自身。
fn high_bytes_ok() -> bool {
    let sp = match parse_spec(b"s") {
        Ok((v, _)) => v,
        Err(_) => return false,
    };
    let mut v = 0x80u32;
    let mut ok = true;
    while v <= 0xff {
        let arg = [v as u8];
        match format_s(&sp, &arg) {
            Ok(out) => {
                if out.len() != 1 || out.byte(0) != Some(v as u8) {
                    ok = false;
                }
            }
            Err(_) => ok = false,
        }
        v += 1;
    }
    let high = high_sample();
    let h = match format_s(&sp, &high) {
        Ok(out) => out.len() == 64 && out.byte(0) == Some(0x80) && out.byte(63) == Some(0xbf),
        Err(_) => false,
    };
    let hex = hexdump(&[0x00, 0x7f, 0x80, 0xff]);
    ok && h && hex.as_str() == "007f80ff"
}

/// 零字节面。
fn nul_ok() -> bool {
    let sp = match parse_spec(b"s") {
        Ok((v, _)) => v,
        Err(_) => return false,
    };
    let a = format_s(&sp, b"ab\0cd").map(|b| b.len()) == Ok(2);
    let b = format_s(&sp, b"\0").map(|b| b.len()) == Ok(0);
    let c = format_s_strict(&sp, b"ab\0cd").err() == Some(PF_EBYTE);
    let d = format_s_strict(&sp, b"abc").map(|b| b.len()) == Ok(3);
    let e = first_nul(b"abc") == None && first_nul(b"a\0b") == Some(1);
    let wide = match parse_spec(b"5s") {
        Ok((v, _)) => v,
        Err(_) => return false,
    };
    let f = format_s(&wide, b"a\0bc").map(|b| b.len()) == Ok(5);
    a && b && c && d && e && f
}

/// 填充面。
fn pad_ok() -> bool {
    let right = match parse_spec(b"5s") {
        Ok((v, _)) => v,
        Err(_) => return false,
    };
    let left = match parse_spec(b"-5s") {
        Ok((v, _)) => v,
        Err(_) => return false,
    };
    let a = match format_s(&right, b"ab") {
        Ok(o) => o.as_slice() == b"   ab" && o.count(b' ') == 3,
        Err(_) => false,
    };
    let b = match format_s(&left, b"ab") {
        Ok(o) => o.as_slice() == b"ab   " && o.count(b' ') == 3,
        Err(_) => false,
    };
    let c = match format_s(&right, b"abcdefg") {
        Ok(o) => o.as_slice() == b"abcdefg" && o.len() == 7,
        Err(_) => false,
    };
    let p = match parse_spec(b".2s") {
        Ok((v, _)) => v,
        Err(_) => return false,
    };
    let d = match format_s(&p, b"abcdef") {
        Ok(o) => o.as_slice() == b"ab",
        Err(_) => false,
    };
    let e = match format_c(&right, b"Z") {
        Ok(o) => o.as_slice() == b"    Z",
        Err(_) => false,
    };
    a && b && c && d && e
}

/// 遍历面。
fn walk_ok() -> bool {
    let a = match walk(b"[%s][%s]", &[b"ab", b"cd"]) {
        Ok(o) => o.as_slice() == b"[ab][cd]",
        Err(_) => false,
    };
    let b = match walk(b"100%%", &[]) {
        Ok(o) => o.as_slice() == b"100%",
        Err(_) => false,
    };
    let c = walk(b"%s %s", &[b"x"]).err() == Some(PF_EARG);
    let d = walk(b"%d", &[b"7"]).err() == Some(PF_ECONV);
    let e = match walk(b"a\xffb", &[]) {
        Ok(o) => o.len() == 3 && o.byte(1) == Some(0xff),
        Err(_) => false,
    };
    let f = match walk(b"%c%c", &[b"ab", b"c"]) {
        Ok(o) => o.as_slice() == b"ac",
        Err(_) => false,
    };
    let g = walk(b"%s", &[]).err() == Some(PF_EARG);
    let h = walk(&[b'%', 0], &[]).err() == Some(PF_EBYTE);
    a && b && c && d && e && f && g && h
}

/// 规格错误面。
fn spec_err_ok() -> bool {
    let a = parse_spec(b"d").err() == Some(PF_ECONV);
    let b = parse_spec(b"ls").err() == Some(PF_ECONV);
    let c = parse_spec(b"").err() == Some(PF_ESPEC);
    let d = parse_spec(b"+ s").err() == Some(PF_EFLAG);
    let e = parse_spec(b"#s").err() == Some(PF_EFLAG);
    let f = parse_spec(b"--s").err() == Some(PF_EFLAG);
    let g = parse_spec(b"5").err() == Some(PF_ESPEC);
    a && b && c && d && e && f && g
}

/// 容量面。
fn capacity_ok() -> bool {
    let mut b = Buf::new();
    let a = b.pad(BUF_CAP, b'x').is_ok() && b.len() == BUF_CAP && b.spare() == 0;
    let c = b.push(b'y').err() == Some(PF_EOVER);
    let mut d = Buf::new();
    let e = d.push_bytes(&[b'z'; BUF_CAP + 1]).err() == Some(PF_EOVER);
    let f = walk(&[b'q'; BUF_CAP + 2], &[]).err() == Some(PF_EOVER);
    let g = Buf::new().len() == 0 && Buf::new().is_empty();
    a && c && e && f && g
}

/// 账本闭环。
fn ledger_ok() -> bool {
    let l = build_ledger();
    if l.audit() != Ok(()) {
        return false;
    }
    if l.len() != SAMPLES || l.ok_count() != 8 || l.err_count() != 6 {
        return false;
    }
    let s1 = l.seal();
    if s1 != build_ledger().seal() {
        return false;
    }
    let mut l2 = build_ledger();
    let root = match l2.freeze() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if root != s1 || !l2.is_frozen() {
        return false;
    }
    let replay = match l2.replay() {
        Ok(v) => v == root,
        Err(_) => false,
    };
    if !replay {
        return false;
    }
    if l2.register(Sample::blank()) != Err(PF_ESLOT) {
        return false;
    }
    l2.rejects() == 1
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = spec_ok_face() && width_prec_ok();
    out[1] = transparency_ok() && high_bytes_ok();
    out[2] = nul_ok() && pad_ok();
    out[3] = walk_ok() && spec_err_ok() && capacity_ok() && ledger_ok();
    out
}

/// 判据集合。
pub fn run_unx11574_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11574.printf_s");

    s.add("PF01-规格解析", spec_ok_face(), "static");
    s.add("PF02-宽度与精度", width_prec_ok(), "static");
    s.add("PF03-高位区间", high_bytes_ok(), "static");
    s.add("PF04-透明位图", transparency_ok(), "static");
    s.add("PF05-零字节截断", nul_ok(), "static");
    s.add("PF06-填充方位", pad_ok(), "static");
    s.add("PF07-整串遍历", walk_ok(), "static");
    s.add("PF08-规格错误码", spec_err_ok(), "static");
    s.add("PF09-缓冲容量", capacity_ok(), "static");
    s.add(
        "PF10-参数耗用",
        {
            let a = walk(b"%s%s%s", &[b"a", b"b", b"c"]).map(|o| o.len()) == Ok(3);
            let b = walk(b"%s", &[b"a", b"b"]).map(|o| o.len()) == Ok(1);
            let c = walk(b"", &[b"a"]).map(|o| o.len()) == Ok(0);
            let d = match walk(b"%5c", &[b"x"]) {
                Ok(o) => o.as_slice() == b"    x",
                Err(_) => false,
            };
            a && b && c && d
        },
        "static",
    );
    s.add("PF11-账本对账", build_ledger().audit() == Ok(()), "static");
    s.add(
        "PF12-冻结指纹",
        {
            let mut l = build_ledger();
            match l.freeze() {
                Ok(r) => r == l.seal() && l.is_frozen() && l.freeze() == Err(PF_EBAL),
                Err(_) => false,
            }
        },
        "static",
    );
    s.add(
        "PF13-冻结重放四注入",
        {
            let four = inject_four();
            four.get(0) == Some(&true)
                && four.get(1) == Some(&true)
                && four.get(2) == Some(&true)
                && four.get(3) == Some(&true)
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_basic() {
        let (sp, n) = parse_spec(b"8s").expect("s");
        assert_eq!(sp.width, 8);
        assert_eq!(sp.conv, b's');
        assert_eq!(n, 2);
        let (sp2, _) = parse_spec(b"-12.3s").expect("s");
        assert!(sp2.minus);
        assert_eq!(sp2.width, 12);
        assert_eq!(sp2.prec, 3);
        assert!(sp2.has_prec);
        let (sp3, _) = parse_spec(b"0-s").expect("s");
        assert!(sp3.minus);
        assert!(!sp3.zero);
    }

    #[test]
    fn spec_limits() {
        assert_eq!(parse_spec(b"128s").map(|(s, _)| s.width), Ok(128));
        assert_eq!(parse_spec(b"129s").err(), Some(PF_EWIDE));
        assert_eq!(parse_spec(b".64s").map(|(s, _)| s.prec), Ok(64));
        assert_eq!(parse_spec(b".65s").err(), Some(PF_EPREC));
        assert_eq!(parse_spec(b"*s").err(), Some(PF_ESPEC));
    }

    #[test]
    fn spec_conflicts() {
        assert_eq!(parse_spec(b"+ s").err(), Some(PF_EFLAG));
        assert_eq!(parse_spec(b"#s").err(), Some(PF_EFLAG));
        assert_eq!(parse_spec(b"--s").err(), Some(PF_EFLAG));
        assert_eq!(parse_spec(b"d").err(), Some(PF_ECONV));
        assert_eq!(parse_spec(b"ls").err(), Some(PF_ECONV));
    }

    #[test]
    fn formats_plain() {
        let sp = parse_spec(b"s").expect("s").0;
        assert_eq!(format_s(&sp, b"hello").map(|b| b.len()), Ok(5));
        assert_eq!(format_s(&sp, b"").map(|b| b.len()), Ok(0));
    }

    #[test]
    fn formats_width() {
        let right = parse_spec(b"5s").expect("s").0;
        assert_eq!(format_s(&right, b"ab").map(|b| b.text()), Ok(String::from("   ab")));
        let left = parse_spec(b"-5s").expect("s").0;
        assert_eq!(format_s(&left, b"ab").map(|b| b.text()), Ok(String::from("ab   ")));
        let exact = parse_spec(b"2s").expect("s").0;
        assert_eq!(format_s(&exact, b"abc").map(|b| b.text()), Ok(String::from("abc")));
    }

    #[test]
    fn formats_precision() {
        let p = parse_spec(b".2s").expect("s").0;
        assert_eq!(format_s(&p, b"abcdef").map(|b| b.text()), Ok(String::from("ab")));
        let p0 = parse_spec(b".0s").expect("s").0;
        assert_eq!(format_s(&p0, b"abc").map(|b| b.len()), Ok(0));
        let pw = parse_spec(b"5.1s").expect("s").0;
        assert_eq!(format_s(&pw, b"abc").map(|b| b.text()), Ok(String::from("    a")));
    }

    #[test]
    fn nul_truncates() {
        let sp = parse_spec(b"s").expect("s").0;
        assert_eq!(format_s(&sp, b"ab\0cd").map(|b| b.len()), Ok(2));
        assert_eq!(format_s_strict(&sp, b"ab\0cd").err(), Some(PF_EBYTE));
        assert_eq!(first_nul(b"a\0b"), Some(1));
        assert_eq!(first_nul(b"abc"), None);
    }

    #[test]
    fn high_bytes_are_transparent() {
        assert_eq!(transparent_count(b"s"), 128);
        assert_eq!(transparent_count(b".1s"), 128);
        assert!(transparent_mask(b"s").iter().all(|v| *v));
        assert!(transparent_at(b"s", 0x80));
        assert!(transparent_at(b"s", 0xff));
        assert!(!transparent_at(b"s", 0x00));
    }

    #[test]
    fn all_256_bytes_roundtrip() {
        let sp = parse_spec(b"s").expect("s").0;
        let mut v = 1u32;
        while v <= 0xff {
            let arg = [v as u8];
            let out = format_s(&sp, &arg).expect("f");
            assert_eq!(out.len(), 1);
            assert_eq!(out.byte(0), Some(v as u8));
            v += 1;
        }
    }

    #[test]
    fn walk_formats() {
        assert_eq!(
            walk(b"[%s][%s]", &[b"ab", b"cd"]).map(|o| o.text()),
            Ok(String::from("[ab][cd]"))
        );
        assert_eq!(walk(b"100%%", &[]).map(|o| o.text()), Ok(String::from("100%")));
        assert_eq!(walk(b"%s", &[]).err(), Some(PF_EARG));
        assert_eq!(walk(b"%d", &[b"1"]).err(), Some(PF_ECONV));
        assert_eq!(walk(&[0u8], &[]).err(), Some(PF_EBYTE));
    }

    #[test]
    fn walk_literal_high_bytes() {
        let fmt = [b'a', 0x80, b'b', 0xff];
        let out = walk(&fmt, &[]).expect("w");
        assert_eq!(out.len(), 4);
        assert_eq!(out.byte(1), Some(0x80));
        assert_eq!(out.byte(3), Some(0xff));
    }

    #[test]
    fn char_conversion() {
        let sp = parse_spec(b"c").expect("s").0;
        assert_eq!(format_c(&sp, b"ABC").map(|b| b.len()), Ok(1));
        assert_eq!(format_c(&sp, b"").err(), Some(PF_EARG));
        let wide = parse_spec(b"4c").expect("s").0;
        assert_eq!(format_c(&wide, b"Z").map(|b| b.text()), Ok(String::from("   Z")));
    }

    #[test]
    fn buffer_bounds() {
        let mut b = Buf::new();
        assert!(b.pad(BUF_CAP, b'x').is_ok());
        assert_eq!(b.len(), BUF_CAP);
        assert_eq!(b.push(b'y').err(), Some(PF_EOVER));
        assert_eq!(walk(&[b'q'; BUF_CAP + 2], &[]).err(), Some(PF_EOVER));
    }

    #[test]
    fn hexdump_shapes() {
        assert_eq!(hexdump(&[0x00, 0x7f, 0x80, 0xff]).as_str(), "007f80ff");
        assert_eq!(hexdump(&[]).as_str(), "");
        assert_eq!(hexdump(&[0x0a]).as_str(), "0a");
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.sample(0).map(|s| s.value), Some(8));
        assert_eq!(l.sample(5).map(|s| s.value), Some(128));
        assert_eq!(l.sample(8).map(|s| s.code), Some(PF_EWIDE));
        assert_eq!(l.sample(12).map(|s| s.code), Some(PF_EFLAG));
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = l.freeze().expect("freeze");
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(PF_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(PF_ESLOT));
        assert_eq!(l.rejects(), 2);
    }

    #[test]
    fn injections() {
        let four = inject_four();
        assert!(four[0] && four[1] && four[2] && four[3]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11574_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert_eq!(s.len(), 13);
    }
}
