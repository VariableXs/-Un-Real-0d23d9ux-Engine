//! UNX-F11613 对齐违例（p_vaddr 页对齐约束）拒收判据
//!
//! 镜面：ELF 装载段的对齐不是一条装饰性字段，而是 mmap 能否把文件页
//! 直接映射进地址空间的硬约束。内核与 ld.so 共用同一套判据：
//!
//! 1. `p_align` 只能是 0、1 或 2 的幂；
//! 2. 对 PT_LOAD 而言，`p_align` 若大于 1，则不得小于页字节；
//! 3. `p_vaddr ≡ p_offset (mod p_align)`；
//! 4. 即便 `p_align` 小于页，`p_vaddr` 与 `p_offset` 也必须页同余；
//! 5. 所有 PT_LOAD 的装入偏置（页底下标之差）必须一致；
//! 6. 各段按页取整后的内存区间不得重叠；
//! 7. `p_vaddr` 不得超过用户地址上界，段末不得溢出 u64。
//!
//! 本单把上述七条收成一条固定次序的闸门链，并对十四槽语料逐条对判：
//! 八活六死——非幂一、页小一、不同余一、页偏一、重叠一、偏置分裂一。
//!
//! 次序的要害在于「先局部后全局」：单段字段的错误先于跨段一致性错误
//! 暴露，跨段偏置错误又先于重叠错误暴露。这样同一个畸形映像只会得到
//! 一条最贴近根因的诊断，而不是随机一条。
//!
//! 闸门次序：名称 → 页算术 → 单段通过面 → 幂与页小 → 同余 → 页偏 →
//! 统一偏置 → 重叠 → 越界 → 类型与空图 → 文案 → 语料汇总 → 冻结重放。
//!
//! 契约：纯 `no_std` 函数式代码，无 `unsafe`，无 `unwrap`/`expect`/
//! `panic!`，失败一律 `Result<_, u32>`；只读、无全局态。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 本单段基址。
pub const SEG_BASE: u32 = 0xfbc0;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// PT_LOAD。
pub const PT_LOAD: u32 = 1;

/// PT_DYNAMIC。
pub const PT_DYNAMIC: u32 = 2;

/// PT_INTERP。
pub const PT_INTERP: u32 = 3;

/// 页字节。
pub const PAGE: u64 = 0x1000;

/// 页内掩码。
pub const PMASK: u64 = PAGE - 1;

/// 用户地址上界（含）。
pub const USER_LIMIT: u64 = 0x0000_7fff_ffff_f000;

/// 映像最大段数。
pub const MAX_SEGS: usize = 4;

/// ENOEXEC。
pub const ENOEXEC: i32 = 8;

/// EINVAL。
pub const EINVAL: i32 = 22;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 对齐字段非法（非 2 的幂或页小）。
pub const AL_EALIGN: u32 = SEG_BASE + 0x01;
/// 不同余（vaddr 与 offset 模 align 或模页不等）。
pub const AL_ECONG: u32 = SEG_BASE + 0x02;
/// 页区间重叠。
pub const AL_EOVER: u32 = SEG_BASE + 0x03;
/// 装入偏置分裂。
pub const AL_EBIAS: u32 = SEG_BASE + 0x04;
/// 越界（超用户上界或段末溢出）。
pub const AL_EHIGH: u32 = SEG_BASE + 0x05;
/// 类型错（非 PT_LOAD 或空图）。
pub const AL_ETYPE: u32 = SEG_BASE + 0x06;
/// 状态违例。
pub const AL_ESTATE: u32 = SEG_BASE + 0x07;
/// 红档。
pub const AL_EVER: u32 = SEG_BASE + 0x08;
/// 账不平。
pub const AL_EBAL: u32 = SEG_BASE + 0x09;
/// 槽错位。
pub const AL_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const AL_EVALUE: u32 = SEG_BASE + 0x0b;
/// 封存违例。
pub const AL_ESEAL: u32 = SEG_BASE + 0x0c;
/// 未观测。
pub const AL_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 对齐算术。
pub const OP_ALIGN: u32 = 1;
/// 同余。
pub const OP_CONGRUENT: u32 = 2;
/// 页偏移。
pub const OP_PAGE: u32 = 3;
/// 重叠。
pub const OP_OVER: u32 = 4;
/// 偏置。
pub const OP_BIAS: u32 = 5;
/// 文案。
pub const OP_TEXT: u32 = 6;
/// 汇总。
pub const OP_REPORT: u32 = 7;

// ---------------------------------------------------------------------------
// 指纹
// ---------------------------------------------------------------------------

/// FNV-1a 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 质数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 折一个字节。
pub const fn fold_byte(h: u64, b: u8) -> u64 {
    let x = h ^ (b as u64);
    x.wrapping_mul(FNV_PRIME)
}

/// 折一个字。
pub const fn fold_word(h: u64, w: u64) -> u64 {
    let mut x = h;
    let mut i = 0u32;
    while i < 8 {
        x = fold_byte(x, ((w >> (i * 8)) & 0xff) as u8);
        i += 1;
    }
    x
}

/// 十进制文本。
pub fn u32_text(v: u32) -> String {
    let mut buf = [0u8; 10];
    let mut n = v;
    let mut i = 10usize;
    if n == 0 {
        return String::from("0");
    }
    while n > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    let mut s = String::new();
    while i < 10 {
        if let Some(b) = buf.get(i) {
            s.push(*b as char);
        }
        i += 1;
    }
    s
}

/// 十六进制文本（带 `0x`，小写）。
pub fn hex_text(v: u64) -> String {
    let mut buf = [0u8; 16];
    let mut n = v;
    let mut i = 16usize;
    if n == 0 {
        return String::from("0x0");
    }
    while n > 0 && i > 0 {
        i -= 1;
        let d = (n & 0xf) as u8;
        buf[i] = if d < 10 { b'0' + d } else { b'a' + (d - 10) };
        n >>= 4;
    }
    let mut s = String::from("0x");
    while i < 16 {
        if let Some(b) = buf.get(i) {
            s.push(*b as char);
        }
        i += 1;
    }
    s
}

// ---------------------------------------------------------------------------
// 页算术
// ---------------------------------------------------------------------------

/// 2 的幂（0 不算）。
pub const fn pow2(v: u64) -> bool {
    v != 0 && (v & (v - 1)) == 0
}

/// 向下取页。
pub const fn page_floor(v: u64) -> u64 {
    v & !PMASK
}

/// 向上取页（0 仍为 0）。
pub const fn page_ceil(v: u64) -> u64 {
    if v == 0 {
        return 0;
    }
    let lo = page_floor(v);
    if lo == v {
        v
    } else {
        lo + PAGE
    }
}

/// `align` 为 0/1 时视为无约束，否则按模判定。
pub const fn congruent(vaddr: u64, off: u64, align: u64) -> bool {
    if align <= 1 {
        return true;
    }
    vaddr % align == off % align
}

/// 页同余：mmap 的硬要求。
pub const fn page_congruent(vaddr: u64, off: u64) -> bool {
    (vaddr & PMASK) == (off & PMASK)
}

/// 段偏置：页底下标之差。
pub const fn bias_of(vaddr: u64, off: u64) -> u64 {
    page_floor(vaddr).wrapping_sub(page_floor(off))
}

/// 校验对齐值本身（不含同余）。
pub const fn align_shape(align: u64) -> bool {
    if align <= 1 {
        return true;
    }
    pow2(align) && align >= PAGE
}

// ---------------------------------------------------------------------------
// 段视图
// ---------------------------------------------------------------------------

/// 一个装载段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seg {
    /// 类型。
    pub p_type: u32,
    /// 文件偏移。
    pub off: u64,
    /// 虚拟地址。
    pub vaddr: u64,
    /// 文件字节。
    pub filesz: u64,
    /// 内存字节。
    pub memsz: u64,
    /// 对齐。
    pub align: u64,
}

/// 便于外部按 ELF 习惯读取文件字节。
impl Seg {
    /// 构造。
    pub const fn new(p_type: u32, off: u64, vaddr: u64, filesz: u64, memsz: u64, align: u64) -> Seg {
        Seg { p_type, off, vaddr, filesz: filesz, memsz, align }
    }

    /// 文本段。
    pub const fn text() -> Seg {
        Seg::new(PT_LOAD, 0, 0x400000, 0x100, 0x100, PAGE)
    }

    /// 只读段。
    pub const fn rodata() -> Seg {
        Seg::new(PT_LOAD, 0x1000, 0x401000, 0x100, 0x100, PAGE)
    }

    /// 数据段。
    pub const fn data() -> Seg {
        Seg::new(PT_LOAD, 0x2000, 0x402000, 0x100, 0x100, PAGE)
    }

    /// 是否装载段。
    pub const fn loadable(&self) -> bool {
        self.p_type == PT_LOAD
    }

    /// 段末（文件口径）。
    pub const fn file_end(&self) -> Option<u64> {
        self.off.checked_add(self.filesz)
    }

    /// 段末（内存口径）。
    pub const fn mem_end(&self) -> Option<u64> {
        self.vaddr.checked_add(self.memsz)
    }

    /// 页区间 `[lo, hi)`。
    pub const fn page_bounds(&self) -> (u64, u64) {
        let lo = page_floor(self.vaddr);
        let end = match self.mem_end() {
            Some(v) => v,
            None => return (lo, u64::MAX & !PMASK),
        };
        (lo, page_ceil(end))
    }

    /// 偏置。
    pub const fn bias(&self) -> u64 {
        bias_of(self.vaddr, self.off)
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.p_type as u64);
        h = fold_word(h, self.off);
        h = fold_word(h, self.vaddr);
        h = fold_word(h, self.filesz);
        h = fold_word(h, self.memsz);
        h = fold_word(h, self.align);
        h
    }
}

// ---------------------------------------------------------------------------
// 映像
// ---------------------------------------------------------------------------

/// 至多四段的装载映像。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Image {
    /// 段表。
    pub segs: [Seg; MAX_SEGS],
    /// 有效段数。
    pub count: u8,
}

impl Image {
    /// 空图。
    pub const fn new() -> Image {
        Image { segs: [Seg::new(0, 0, 0, 0, 0, 1); MAX_SEGS], count: 0 }
    }

    /// 一段图。
    pub const fn one(a: Seg) -> Image {
        let mut segs = [Seg::new(0, 0, 0, 0, 0, 1); MAX_SEGS];
        segs[0] = a;
        Image { segs, count: 1 }
    }

    /// 两段图。
    pub const fn two(a: Seg, b: Seg) -> Image {
        let mut segs = [Seg::new(0, 0, 0, 0, 0, 1); MAX_SEGS];
        segs[0] = a;
        segs[1] = b;
        Image { segs, count: 2 }
    }

    /// 三段图。
    pub const fn three(a: Seg, b: Seg, c: Seg) -> Image {
        let mut segs = [Seg::new(0, 0, 0, 0, 0, 1); MAX_SEGS];
        segs[0] = a;
        segs[1] = b;
        segs[2] = c;
        Image { segs, count: 3 }
    }

    /// 追加一段。
    pub fn push(&mut self, s: Seg) -> Result<(), u32> {
        if !s.loadable() {
            return Err(AL_ETYPE);
        }
        let i = self.count as usize;
        if i >= MAX_SEGS {
            return Err(AL_EBAL);
        }
        if let Some(dst) = self.segs.get_mut(i) {
            *dst = s;
            self.count = (i + 1) as u8;
            Ok(())
        } else {
            Err(AL_EBAL)
        }
    }

    /// 段数。
    pub const fn len(&self) -> usize {
        self.count as usize
    }

    /// 是否空图。
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// 取一段。
    pub fn get(&self, i: usize) -> Option<&Seg> {
        if i >= self.len() {
            return None;
        }
        self.segs.get(i)
    }

    /// 统一偏置；存在分裂或空图时为 `None`。
    pub fn unified_bias(&self) -> Option<u64> {
        let first = self.get(0)?;
        let b = first.bias();
        let mut i = 1usize;
        while i < self.len() {
            let s = self.get(i)?;
            if s.bias() != b {
                return None;
            }
            i += 1;
        }
        Some(b)
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.count as u64);
        let mut i = 0usize;
        while i < self.len() {
            if let Some(s) = self.get(i) {
                h = fold_word(h, s.seal());
            }
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 判定
// ---------------------------------------------------------------------------

/// 判面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 通过。
    Ok,
    /// 非 2 的幂。
    Not2,
    /// 页小。
    Small,
    /// 不同余。
    Cong,
    /// 页偏。
    Skew,
    /// 重叠。
    Over,
    /// 偏置分裂。
    Bias,
    /// 越界。
    High,
    /// 类型错。
    Type,
    /// 空图。
    Empty,
}

impl Fault {
    /// 名称。
    pub const fn name(&self) -> &'static str {
        match self {
            Fault::Ok => "ok",
            Fault::Not2 => "not-pow2",
            Fault::Small => "subpage",
            Fault::Cong => "congruence",
            Fault::Skew => "page-skew",
            Fault::Over => "overlap",
            Fault::Bias => "bias-split",
            Fault::High => "above-limit",
            Fault::Type => "not-load",
            Fault::Empty => "empty-image",
        }
    }

    /// 错误码；通过为 0。
    pub const fn code(&self) -> u32 {
        match self {
            Fault::Ok => 0,
            Fault::Not2 | Fault::Small => AL_EALIGN,
            Fault::Cong | Fault::Skew => AL_ECONG,
            Fault::Over => AL_EOVER,
            Fault::Bias => AL_EBIAS,
            Fault::High => AL_EHIGH,
            Fault::Type | Fault::Empty => AL_ETYPE,
        }
    }

    /// errno。
    pub const fn errno(&self) -> i32 {
        match self {
            Fault::Ok => 0,
            Fault::Type | Fault::Empty => ENOEXEC,
            _ => EINVAL,
        }
    }

    /// 是否拒收面。
    pub const fn alive(&self) -> bool {
        !matches!(self, Fault::Ok)
    }
}

/// 单段局部判定（不含跨段项）。
pub fn seg_fault(s: &Seg) -> Fault {
    if !s.loadable() {
        return Fault::Type;
    }
    if s.align > 1 && !pow2(s.align) {
        return Fault::Not2;
    }
    if s.align > 1 && s.align < PAGE {
        return Fault::Small;
    }
    if !congruent(s.vaddr, s.off, s.align) {
        return Fault::Cong;
    }
    if !page_congruent(s.vaddr, s.off) {
        return Fault::Skew;
    }
    if s.vaddr > USER_LIMIT {
        return Fault::High;
    }
    if s.mem_end().is_none() {
        return Fault::High;
    }
    Fault::Ok
}

/// 找第一对按页重叠的段。
pub fn overlap_pair(img: &Image) -> Option<(usize, usize)> {
    let mut i = 0usize;
    while i < img.len() {
        let a = match img.get(i) {
            Some(v) => *v,
            None => return None,
        };
        let (a0, a1) = a.page_bounds();
        let mut j = i + 1;
        while j < img.len() {
            if let Some(b) = img.get(j) {
                let (b0, b1) = b.page_bounds();
                if a0 < b1 && b0 < a1 {
                    return Some((i, j));
                }
            }
            j += 1;
        }
        i += 1;
    }
    None
}

/// 找第一处偏置分裂：`(下标, 实际, 基准)`。
pub fn bias_split(img: &Image) -> Option<(usize, u64, u64)> {
    let first = img.get(0)?;
    let want = first.bias();
    let mut i = 1usize;
    while i < img.len() {
        let s = match img.get(i) {
            Some(v) => *v,
            None => return None,
        };
        let got = s.bias();
        if got != want {
            return Some((i, got, want));
        }
        i += 1;
    }
    None
}

/// 全图判定：单段 → 偏置 → 重叠。
pub fn validate(img: &Image) -> Fault {
    if img.is_empty() {
        return Fault::Empty;
    }
    let mut i = 0usize;
    while i < img.len() {
        let s = match img.get(i) {
            Some(v) => *v,
            None => return Fault::Empty,
        };
        let f = seg_fault(&s);
        if f != Fault::Ok {
            return f;
        }
        i += 1;
    }
    if bias_split(img).is_some() {
        return Fault::Bias;
    }
    if overlap_pair(img).is_some() {
        return Fault::Over;
    }
    Fault::Ok
}

/// 找最贴近根因的一段（用于文案）。
fn culprit(img: &Image) -> Seg {
    let mut i = 0usize;
    while i < img.len() {
        if let Some(s) = img.get(i) {
            if seg_fault(s) != Fault::Ok {
                return *s;
            }
        }
        i += 1;
    }
    match img.get(0) {
        Some(v) => *v,
        None => Seg::new(0, 0, 0, 0, 0, 1),
    }
}

/// 单行诊断文案。
pub fn message(f: Fault, img: &Image) -> String {
    let mut s = String::new();
    match f {
        Fault::Ok => {
            s.push_str("ok: segments=");
            s.push_str(&u32_text(img.len() as u32));
            s.push_str(" bias=");
            match img.unified_bias() {
                Some(b) => s.push_str(&hex_text(b)),
                None => s.push_str("0x0"),
            }
            s.push('\n');
        }
        Fault::Not2 => {
            let g = culprit(img);
            s.push_str("./prog: bad PT_LOAD: p_align=");
            s.push_str(&hex_text(g.align));
            s.push_str(" not a power of two\n");
        }
        Fault::Small => {
            let g = culprit(img);
            s.push_str("./prog: bad PT_LOAD: p_align=");
            s.push_str(&hex_text(g.align));
            s.push_str(" below page size ");
            s.push_str(&hex_text(PAGE));
            s.push('\n');
        }
        Fault::Cong => {
            let g = culprit(img);
            s.push_str("./prog: bad PT_LOAD: p_vaddr=");
            s.push_str(&hex_text(g.vaddr));
            s.push_str(" not congruent with p_offset=");
            s.push_str(&hex_text(g.off));
            s.push_str(" mod p_align=");
            s.push_str(&hex_text(g.align));
            s.push('\n');
        }
        Fault::Skew => {
            let g = culprit(img);
            s.push_str("./prog: bad PT_LOAD: p_vaddr=");
            s.push_str(&hex_text(g.vaddr));
            s.push_str(" not page-congruent with p_offset=");
            s.push_str(&hex_text(g.off));
            s.push_str(" (mod ");
            s.push_str(&hex_text(PAGE));
            s.push_str(")\n");
        }
        Fault::High => {
            let g = culprit(img);
            s.push_str("./prog: bad PT_LOAD: p_vaddr=");
            s.push_str(&hex_text(g.vaddr));
            if g.vaddr > USER_LIMIT {
                s.push_str(" above user limit ");
                s.push_str(&hex_text(USER_LIMIT));
            } else {
                s.push_str(" end overflows u64");
            }
            s.push('\n');
        }
        Fault::Type => {
            let g = culprit(img);
            s.push_str("./prog: bad segment: p_type=");
            s.push_str(&hex_text(g.p_type as u64));
            s.push_str(" not PT_LOAD\n");
        }
        Fault::Empty => {
            s.push_str("./prog: empty image: no loadable segments\n");
        }
        Fault::Over => match overlap_pair(img) {
            Some((i, j)) => {
                let a = match img.get(i) {
                    Some(v) => *v,
                    None => return String::from("./prog: PT_LOAD segments overlap\n"),
                };
                let b = match img.get(j) {
                    Some(v) => *v,
                    None => return String::from("./prog: PT_LOAD segments overlap\n"),
                };
                let (a0, a1) = a.page_bounds();
                let (b0, b1) = b.page_bounds();
                s.push_str("./prog: PT_LOAD segments overlap: [");
                s.push_str(&hex_text(a0));
                s.push_str(",");
                s.push_str(&hex_text(a1));
                s.push_str(") and [");
                s.push_str(&hex_text(b0));
                s.push_str(",");
                s.push_str(&hex_text(b1));
                s.push_str(")\n");
            }
            None => {
                s.push_str("./prog: PT_LOAD segments overlap\n");
            }
        },
        Fault::Bias => match bias_split(img) {
            Some((i, got, want)) => {
                s.push_str("./prog: inconsistent load bias: segment ");
                s.push_str(&u32_text(i as u32));
                s.push_str(" has ");
                s.push_str(&hex_text(got));
                s.push_str(", first ");
                s.push_str(&hex_text(want));
                s.push('\n');
            }
            None => {
                s.push_str("./prog: inconsistent load bias\n");
            }
        },
    }
    s
}

/// 短标签。
pub fn short(f: Fault) -> &'static str {
    f.name()
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 一槽。
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    /// 映像。
    pub img: Image,
    /// 期望判面。
    pub fault: Fault,
    /// 备注。
    pub note: &'static str,
}

impl Slot {
    /// 构造。
    pub const fn new(img: Image, fault: Fault, note: &'static str) -> Slot {
        Slot { img, fault, note }
    }
}

/// 十四槽语料：八活六死。
pub const CASES: [Slot; CORPUS] = [
    Slot::new(Image::one(Seg::text()), Fault::Ok, "text-page"),
    Slot::new(Image::one(Seg::rodata()), Fault::Ok, "rodata-page"),
    Slot::new(Image::one(Seg::data()), Fault::Ok, "data-page"),
    Slot::new(Image::two(Seg::text(), Seg::rodata()), Fault::Ok, "text+rodata"),
    Slot::new(Image::two(Seg::text(), Seg::data()), Fault::Ok, "text+data"),
    Slot::new(Image::one(Seg::new(PT_LOAD, 0, 0x400000, 0x100, 0x100, 0x20_0000)), Fault::Ok, "align-2m"),
    Slot::new(Image::one(Seg::new(PT_LOAD, 0x1234, 0x401234, 0x10, 0x10, 1)), Fault::Ok, "align-one"),
    Slot::new(Image::one(Seg::new(PT_LOAD, 0x3000, 0x403000, 0x10, 0x10, 0)), Fault::Ok, "align-zero"),
    Slot::new(Image::one(Seg::new(PT_LOAD, 0, 0x400000, 0x10, 0x10, 0x1800)), Fault::Not2, "align-3x-page"),
    Slot::new(Image::one(Seg::new(PT_LOAD, 0x800, 0x400800, 0x10, 0x10, 0x800)), Fault::Small, "align-subpage"),
    Slot::new(Image::one(Seg::new(PT_LOAD, 0, 0x401800, 0x10, 0x10, PAGE)), Fault::Cong, "vaddr-cong"),
    Slot::new(Image::one(Seg::new(PT_LOAD, 0x100, 0x401000, 0x10, 0x10, 1)), Fault::Skew, "vaddr-skew"),
    Slot::new(
        Image::two(Seg::text(), Seg::new(PT_LOAD, 0x800, 0x400800, 0x10, 0x10, PAGE)),
        Fault::Over,
        "overlap-page",
    ),
    Slot::new(
        Image::two(Seg::text(), Seg::new(PT_LOAD, 0x1000, 0x402000, 0x10, 0x10, PAGE)),
        Fault::Bias,
        "bias-split",
    ),
];

/// 取一槽。
pub fn slot_at(i: usize) -> Option<Slot> {
    CASES.get(i).copied()
}

/// 语料是否互异。
pub fn unique() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        let a = match CASES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut j = i + 1;
        while j < CASES.len() {
            let b = match CASES.get(j) {
                Some(v) => *v,
                None => return false,
            };
            if a.img == b.img || a.note == b.note {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 备注全非空。
pub fn notes_ok() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        match CASES.get(i) {
            Some(v) => {
                if v.note.is_empty() {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 语料逐条对判。
pub fn table_ok() -> bool {
    if !unique() || !notes_ok() {
        return false;
    }
    let mut i = 0usize;
    while i < CASES.len() {
        let s = match CASES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if validate(&s.img) != s.fault {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 汇总
// ---------------------------------------------------------------------------

/// 汇总面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    /// 总数。
    pub total: u32,
    /// 通过。
    pub accept: u32,
    /// 拒收。
    pub reject: u32,
    /// 非幂。
    pub not2: u32,
    /// 页小。
    pub small: u32,
    /// 不同余。
    pub cong: u32,
    /// 页偏。
    pub skew: u32,
    /// 重叠。
    pub over: u32,
    /// 偏置。
    pub bias: u32,
    /// 对判命中。
    pub matched: u32,
}

impl Report {
    /// 空面。
    pub const fn new() -> Report {
        Report { total: 0, accept: 0, reject: 0, not2: 0, small: 0, cong: 0, skew: 0, over: 0, bias: 0, matched: 0 }
    }

    /// 是否空。
    pub const fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// 命中率（千分比，四舍五入）。
    pub const fn rate(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        (self.matched * 1000 + self.total / 2) / self.total
    }

    /// 是否全绿。
    pub const fn green(&self) -> bool {
        self.total == CORPUS as u32 && self.matched == self.total
    }

    /// 闸门。
    pub const fn gate(&self) -> Result<(), u32> {
        if self.total != CORPUS as u32 {
            return Err(AL_ENOBS);
        }
        if self.matched != self.total {
            return Err(AL_EVER);
        }
        Ok(())
    }

    /// 计一槽。
    pub fn tally(&mut self, f: Fault) {
        self.total += 1;
        if f == Fault::Ok {
            self.accept += 1;
            return;
        }
        self.reject += 1;
        match f {
            Fault::Not2 => self.not2 += 1,
            Fault::Small => self.small += 1,
            Fault::Cong => self.cong += 1,
            Fault::Skew => self.skew += 1,
            Fault::Over => self.over += 1,
            Fault::Bias => self.bias += 1,
            _ => {}
        }
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("align corpus=");
        s.push_str(&u32_text(self.total));
        s.push_str(" accept=");
        s.push_str(&u32_text(self.accept));
        s.push_str(" reject=");
        s.push_str(&u32_text(self.reject));
        s.push_str(" not2=");
        s.push_str(&u32_text(self.not2));
        s.push_str(" small=");
        s.push_str(&u32_text(self.small));
        s.push_str(" cong=");
        s.push_str(&u32_text(self.cong));
        s.push_str(" skew=");
        s.push_str(&u32_text(self.skew));
        s.push_str(" over=");
        s.push_str(&u32_text(self.over));
        s.push_str(" bias=");
        s.push_str(&u32_text(self.bias));
        s.push_str(" match=");
        s.push_str(&u32_text(self.matched));
        s.push_str(" rate=");
        s.push_str(&u32_text(self.rate()));
        s.push_str("ppm\n");
        s
    }
}

/// 全跑。
pub fn run() -> Result<Report, u32> {
    run_with(None)
}

/// 带红档覆盖的全跑。
pub fn run_with(over: Option<(usize, u32)>) -> Result<Report, u32> {
    if let Some((i, _)) = over {
        if i >= CORPUS {
            return Err(AL_ESLOT);
        }
    }
    let mut r = Report::new();
    let mut i = 0usize;
    while i < CASES.len() {
        let slot = match CASES.get(i) {
            Some(v) => *v,
            None => return Err(AL_ESLOT),
        };
        let got = validate(&slot.img);
        r.tally(got);
        let overridden = match over {
            Some((k, _)) => k == i,
            None => false,
        };
        if got == slot.fault && !overridden {
            r.matched += 1;
        }
        i += 1;
    }
    Ok(r)
}

/// 覆盖率闸门。
pub fn cover(n: usize) -> Result<(), u32> {
    if n == CORPUS {
        Ok(())
    } else {
        Err(AL_ENOBS)
    }
}

// ---------------------------------------------------------------------------
// 校验器（单次）
// ---------------------------------------------------------------------------

/// 单次校验器。
#[derive(Clone, Copy, Debug)]
pub struct Checker {
    used: bool,
    last: u32,
    steps: u32,
}

impl Checker {
    /// 新建。
    pub const fn new() -> Checker {
        Checker { used: false, last: 0, steps: 0 }
    }

    /// 是否已用。
    pub const fn bound(&self) -> bool {
        self.used
    }

    /// 步数。
    pub const fn steps(&self) -> u32 {
        self.steps
    }

    /// 末码。
    pub const fn last(&self) -> u32 {
        self.last
    }

    /// 校验一图。
    pub fn check(&mut self, img: &Image) -> Result<Fault, u32> {
        if self.used {
            self.last = AL_ESTATE;
            return Err(AL_ESTATE);
        }
        self.used = true;
        self.steps = 1;
        let f = validate(img);
        self.last = f.code();
        Ok(f)
    }
}

// ---------------------------------------------------------------------------
// 账本
// ---------------------------------------------------------------------------

/// 一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 槽位。
    pub slot: u32,
    /// 操作码。
    pub op: u32,
    /// 是否成功。
    pub ok: bool,
    /// 错误码。
    pub code: u32,
    /// 数值。
    pub value: u32,
}

impl Sample {
    /// 空格。
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
    /// 新账。
    pub const fn new() -> Ledger {
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(AL_ESEAL);
        }
        if self.len >= SAMPLES {
            return Err(AL_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
            self.len += 1;
            Ok(())
        } else {
            Err(AL_ESLOT)
        }
    }

    /// 格数。
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一格。
    pub fn sample(&self, i: usize) -> Option<&Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i)
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
            return Err(AL_EBAL);
        }
        const VALS: [u32; 8] = [14, 8, 6, 1, 1, 1, 1, 929];
        const CODES: [u32; 6] = [AL_EALIGN, AL_EALIGN, AL_ECONG, AL_ECONG, AL_EOVER, AL_EBIAS];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(AL_EBAL),
            };
            if s.slot as usize != i {
                return Err(AL_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(AL_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(AL_EVALUE);
                        }
                    }
                    None => return Err(AL_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(AL_EVALUE);
                        }
                    }
                    None => return Err(AL_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(AL_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold_word(h, s.slot as u64);
                h = fold_word(h, s.op as u64);
                h = fold_word(h, s.ok as u64);
                h = fold_word(h, s.code as u64);
                h = fold_word(h, s.value as u64);
            }
            i += 1;
        }
        fold_word(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(AL_ESEAL);
        }
        if self.len != SAMPLES {
            return Err(AL_EBAL);
        }
        self.audit()?;
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if !self.frozen {
            return Err(AL_ESTATE);
        }
        let again = self.seal();
        if again != self.root {
            return Err(AL_ESEAL);
        }
        Ok(self.root)
    }

    /// 根。
    pub const fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub const fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒收次数。
    pub const fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 计数格。
fn count_cell(slot: u32, op: u32, v: Result<usize, u32>) -> Sample {
    match v {
        Ok(n) => Sample { slot, op, ok: true, code: 0, value: n as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 错误格。
fn err_cell(slot: u32, op: u32, code: u32) -> Sample {
    Sample { slot, op, ok: false, code, value: 0 }
}

/// 造账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();

    let _ = l.register(count_cell(0, OP_REPORT, r.map(|x| x.total as usize)));
    let _ = l.register(count_cell(1, OP_REPORT, r.map(|x| x.accept as usize)));
    let _ = l.register(count_cell(2, OP_REPORT, r.map(|x| x.reject as usize)));
    let _ = l.register(count_cell(3, OP_ALIGN, r.map(|x| x.not2 as usize)));

    let _ = l.register(count_cell(4, OP_ALIGN, r.map(|x| x.small as usize)));
    let _ = l.register(count_cell(5, OP_CONGRUENT, r.map(|x| x.cong as usize)));
    let _ = l.register(count_cell(6, OP_PAGE, r.map(|x| x.skew as usize)));

    let red = run_with(Some((0, AL_EALIGN)));
    let _ = l.register(count_cell(7, OP_TEXT, red.map(|x| x.rate() as usize)));

    let n2 = CASES[8].fault;
    let _ = l.register(err_cell(8, OP_ALIGN, n2.code()));

    let sm = CASES[9].fault;
    let _ = l.register(err_cell(9, OP_ALIGN, sm.code()));

    let cg = CASES[10].fault;
    let _ = l.register(err_cell(10, OP_CONGRUENT, cg.code()));

    let sk = CASES[11].fault;
    let _ = l.register(err_cell(11, OP_PAGE, sk.code()));

    let ov = CASES[12].fault;
    let _ = l.register(err_cell(12, OP_OVER, ov.code()));

    let bi = CASES[13].fault;
    let _ = l.register(err_cell(13, OP_BIAS, bi.code()));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 名称与段基址。
fn name_ok() -> bool {
    let names = [
        Fault::Ok.name(),
        Fault::Not2.name(),
        Fault::Small.name(),
        Fault::Cong.name(),
        Fault::Skew.name(),
        Fault::Over.name(),
        Fault::Bias.name(),
        Fault::High.name(),
        Fault::Type.name(),
        Fault::Empty.name(),
    ];
    let mut i = 0usize;
    while i < names.len() {
        if names[i].is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < names.len() {
            if names[i] == names[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    AL_EALIGN == SEG_BASE + 0x01
        && AL_ECONG == SEG_BASE + 0x02
        && AL_EOVER == SEG_BASE + 0x03
        && AL_EBIAS == SEG_BASE + 0x04
        && AL_EHIGH == SEG_BASE + 0x05
        && AL_ETYPE == SEG_BASE + 0x06
        && AL_ESTATE == SEG_BASE + 0x07
        && AL_EVER == SEG_BASE + 0x08
        && AL_EBAL == SEG_BASE + 0x09
        && AL_ESLOT == SEG_BASE + 0x0a
        && AL_EVALUE == SEG_BASE + 0x0b
        && AL_ESEAL == SEG_BASE + 0x0c
        && AL_ENOBS == SEG_BASE + 0x0d
        && AL_ECONG != AL_EOVER
        && AL_EVALUE != AL_ESEAL
}

/// 页算术。
fn arithmetic_ok() -> bool {
    let a = pow2(1) && pow2(2) && pow2(PAGE) && pow2(0x20_0000);
    let b = !pow2(0) && !pow2(3) && !pow2(0x1800) && !pow2(0x3000);
    let c = page_floor(0) == 0 && page_floor(0xfff) == 0 && page_floor(0x1000) == 0x1000;
    let d = page_ceil(0) == 0 && page_ceil(1) == PAGE && page_ceil(PAGE) == PAGE;
    let e = page_ceil(PAGE + 1) == 2 * PAGE && page_floor(u64::MAX) == u64::MAX & !PMASK;
    let f = bias_of(0x401000, 0x1000) == 0x400000 && bias_of(0x400800, 0x800) == 0x400000;
    let g = bias_of(0x402000, 0x1000) == 0x401000 && bias_of(0x401000, 0x1000) == bias_of(0x402000, 0x2000);
    a && b && c && d && e && f && g
}

/// 同余。
fn congruent_ok() -> bool {
    let a = congruent(0x400000, 0, PAGE) && congruent(0x401000, 0x1000, PAGE);
    let b = !congruent(0x401800, 0, PAGE) && !congruent(0, 1, PAGE);
    let c = congruent(5, 9, 2) && !congruent(5, 9, 3);
    let d = congruent(1, 2, 0) && congruent(1, 2, 1);
    let e = page_congruent(0x401100, 0x100) && !page_congruent(0x401000, 0x100);
    let f = !congruent(0x400800, 0x100, 0x1800);
    a && b && c && d && e && f
}

/// 单段通过面。
fn seg_ok() -> bool {
    let a = seg_fault(&Seg::text()) == Fault::Ok;
    let b = seg_fault(&Seg::rodata()) == Fault::Ok;
    let c = seg_fault(&Seg::data()) == Fault::Ok;
    let d = seg_fault(&Seg::new(PT_LOAD, 0, 0x400000, 0x100, 0x100, 0x20_0000)) == Fault::Ok;
    let e = seg_fault(&Seg::new(PT_LOAD, 0x1234, 0x401234, 0x10, 0x10, 1)) == Fault::Ok;
    let f = seg_fault(&Seg::new(PT_LOAD, 0x3000, 0x403000, 0x10, 0x10, 0)) == Fault::Ok;
    let g = seg_fault(&Seg::new(PT_LOAD, 0, USER_LIMIT, 0x10, 0x10, PAGE)) == Fault::Ok;
    a && b && c && d && e && f && g
}

/// 幂与页小。
fn not2_ok() -> bool {
    let a = seg_fault(&Seg::new(PT_LOAD, 0, 0x400000, 0x10, 0x10, 0x1800)) == Fault::Not2;
    let b = seg_fault(&Seg::new(PT_LOAD, 1, 0x401001, 0x10, 0x10, 0x1800)) == Fault::Not2;
    let c = seg_fault(&Seg::new(PT_LOAD, 0x800, 0x400800, 0x10, 0x10, 0x800)) == Fault::Small;
    let d = seg_fault(&Seg::new(PT_LOAD, 0x400, 0x400400, 0x10, 0x10, 0x400)) == Fault::Small;
    let e = Fault::Not2.code() == AL_EALIGN && Fault::Small.code() == AL_EALIGN;
    a && b && c && d && e
}

/// 同余闸门。
fn cong_ok() -> bool {
    let a = seg_fault(&Seg::new(PT_LOAD, 0, 0x401800, 0x10, 0x10, PAGE)) == Fault::Cong;
    let b = seg_fault(&Seg::new(PT_LOAD, 0, 0x401000, 0x10, 0x10, 0x2000)) == Fault::Cong;
    let c = seg_fault(&Seg::new(PT_LOAD, 0x1000, 0x403000, 0x10, 0x10, 0x1000)) == Fault::Ok;
    let d = Fault::Cong.code() == AL_ECONG && Fault::Cong.errno() == EINVAL;
    a && b && c && d
}

/// 页偏闸门。
fn page_ok() -> bool {
    let a = seg_fault(&Seg::new(PT_LOAD, 0x100, 0x401000, 0x10, 0x10, 1)) == Fault::Skew;
    let b = seg_fault(&Seg::new(PT_LOAD, 0x100, 0x401100, 0x10, 0x10, 1)) == Fault::Ok;
    let c = seg_fault(&Seg::new(PT_LOAD, 0x800, 0x400800, 0x10, 0x10, PAGE)) == Fault::Ok;
    let d = Fault::Skew.code() == AL_ECONG && Fault::Skew.errno() == EINVAL;
    let e = page_congruent(0x400ffe, 0xffe) && !page_congruent(0x400fff, 0xffe);
    a && b && c && d && e
}

/// 统一偏置。
fn bias_ok() -> bool {
    let ok2 = Image::two(Seg::text(), Seg::rodata());
    let a = ok2.unified_bias() == Some(0x400000);
    let ok3 = Image::three(Seg::text(), Seg::rodata(), Seg::data());
    let b = ok3.unified_bias() == Some(0x400000) && validate(&ok3) == Fault::Ok;
    let split = Image::two(Seg::text(), Seg::new(PT_LOAD, 0x1000, 0x402000, 0x10, 0x10, PAGE));
    let c = validate(&split) == Fault::Bias && split.unified_bias().is_none();
    let d = bias_split(&split) == Some((1, 0x401000, 0x400000));
    let e = Fault::Bias.code() == AL_EBIAS && Image::new().unified_bias().is_none();
    a && b && c && d && e
}

/// 重叠闸门。
fn over_ok() -> bool {
    let over = Image::two(Seg::text(), Seg::new(PT_LOAD, 0x800, 0x400800, 0x10, 0x10, PAGE));
    let a = validate(&over) == Fault::Over && overlap_pair(&over) == Some((0, 1));
    let adj = Image::two(Seg::text(), Seg::new(PT_LOAD, 0x1000, 0x401000, 0x10, 0x10, PAGE));
    let b = validate(&adj) == Fault::Ok && overlap_pair(&adj).is_none();
    let same = Image::two(Seg::text(), Seg::text());
    let c = validate(&same) == Fault::Over;
    let bss = Image::two(
        Seg::text(),
        Seg::new(PT_LOAD, 0x1000, 0x401000, 0, 0, PAGE),
    );
    let d = overlap_pair(&bss).is_none();
    let e = overlap_pair(&Image::one(Seg::text())).is_none();
    a && b && c && d && e
}

/// 越界与上界。
fn bound_ok() -> bool {
    let a = seg_fault(&Seg::new(PT_LOAD, 0, 0x8000_0000_0000, 0x10, 0x10, PAGE)) == Fault::High;
    let b = seg_fault(&Seg::new(PT_LOAD, 0, 0x400000, 0x10, u64::MAX, PAGE)) == Fault::High;
    let c = seg_fault(&Seg::new(PT_LOAD, 0, USER_LIMIT, 0x10, 0x10, PAGE)) == Fault::Ok;
    let d = seg_fault(&Seg::new(PT_LOAD, 0, USER_LIMIT + PAGE, 0x10, 0x10, PAGE)) == Fault::High;
    let e = Fault::High.code() == AL_EHIGH && Fault::High.errno() == EINVAL;
    let f = Seg::new(PT_LOAD, 0, 0x400000, 0x10, u64::MAX, PAGE).mem_end().is_none();
    a && b && c && d && e && f
}

/// 类型与空图。
fn type_ok() -> bool {
    let a = seg_fault(&Seg::new(PT_DYNAMIC, 0x40, 0x400040, 0x10, 0x10, 1)) == Fault::Type;
    let b = validate(&Image::one(Seg::new(PT_INTERP, 0, 0, 0x10, 0x10, 1))) == Fault::Type;
    let c = validate(&Image::new()) == Fault::Empty;
    let d = Fault::Type.code() == AL_ETYPE && Fault::Type.errno() == ENOEXEC;
    let e = Fault::Empty.errno() == ENOEXEC && Fault::Empty.alive();
    let f = Fault::Ok.code() == 0 && !Fault::Ok.alive();
    a && b && c && d && e && f
}

/// 文案。
fn message_ok() -> bool {
    let n2 = Image::one(Seg::new(PT_LOAD, 0, 0x400000, 0x10, 0x10, 0x1800));
    let a = message(Fault::Not2, &n2).as_str()
        == "./prog: bad PT_LOAD: p_align=0x1800 not a power of two\n";
    let sm = Image::one(Seg::new(PT_LOAD, 0x800, 0x400800, 0x10, 0x10, 0x800));
    let b = message(Fault::Small, &sm).as_str()
        == "./prog: bad PT_LOAD: p_align=0x800 below page size 0x1000\n";
    let cg = Image::one(Seg::new(PT_LOAD, 0, 0x401800, 0x10, 0x10, PAGE));
    let c = message(Fault::Cong, &cg).as_str()
        == "./prog: bad PT_LOAD: p_vaddr=0x401800 not congruent with p_offset=0x0 mod p_align=0x1000\n";
    let sk = Image::one(Seg::new(PT_LOAD, 0x100, 0x401000, 0x10, 0x10, 1));
    let d = message(Fault::Skew, &sk).as_str()
        == "./prog: bad PT_LOAD: p_vaddr=0x401000 not page-congruent with p_offset=0x100 (mod 0x1000)\n";
    let e = message(Fault::Ok, &Image::one(Seg::text())).as_str()
        == "ok: segments=1 bias=0x400000\n";
    a && b && c && d && e
}

/// 后半文案。
fn tail_message_ok() -> bool {
    let ov = Image::two(Seg::text(), Seg::new(PT_LOAD, 0x800, 0x400800, 0x10, 0x10, PAGE));
    let a = message(Fault::Over, &ov).as_str()
        == "./prog: PT_LOAD segments overlap: [0x400000,0x401000) and [0x400000,0x401000)\n";
    let bi = Image::two(Seg::text(), Seg::new(PT_LOAD, 0x1000, 0x402000, 0x10, 0x10, PAGE));
    let b = message(Fault::Bias, &bi).as_str()
        == "./prog: inconsistent load bias: segment 1 has 0x401000, first 0x400000\n";
    let hi = Image::one(Seg::new(PT_LOAD, 0, 0x8000_0000_0000, 0x10, 0x10, PAGE));
    let c = message(Fault::High, &hi).as_str()
        == "./prog: bad PT_LOAD: p_vaddr=0x800000000000 above user limit 0x7ffffffff000\n";
    let ty = Image::one(Seg::new(PT_DYNAMIC, 0x40, 0x400040, 0x10, 0x10, 1));
    let d = message(Fault::Type, &ty).as_str()
        == "./prog: bad segment: p_type=0x2 not PT_LOAD\n";
    let e = message(Fault::Empty, &Image::new()).as_str()
        == "./prog: empty image: no loadable segments\n";
    let f = message(Fault::Ok, &Image::two(Seg::text(), Seg::rodata())).as_str()
        == "ok: segments=2 bias=0x400000\n";
    a && b && c && d && e && f
}

/// 语料与汇总。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.total == 14 && r.accept == 8 && r.reject == 6 && r.matched == 14;
    let b = r.not2 == 1 && r.small == 1 && r.cong == 1 && r.skew == 1;
    let c = r.over == 1 && r.bias == 1;
    let d = r.rate() == 1000 && r.green() && r.gate() == Ok(());
    let e = table_ok() && unique() && notes_ok();
    a && b && c && d && e
}

/// 槽号。
fn slots_ok() -> bool {
    let a = match slot_at(0) {
        Some(s) => s.fault == Fault::Ok && s.img.len() == 1,
        None => false,
    };
    let b = match slot_at(3) {
        Some(s) => s.fault == Fault::Ok && s.img.len() == 2,
        None => false,
    };
    let c = match slot_at(13) {
        Some(s) => s.fault == Fault::Bias && validate(&s.img) == Fault::Bias,
        None => false,
    };
    let d = slot_at(CORPUS).is_none();
    let e = cover(CORPUS) == Ok(()) && cover(CORPUS + 1) == Err(AL_ENOBS);
    a && b && c && d && e
}

/// 红档。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, AL_EALIGN))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched == CORPUS as u32 - 1 && r.rate() == 929;
    let b = r.gate() == Err(AL_EVER) && !r.green();
    let c = run_with(Some((CORPUS, AL_EALIGN))).err() == Some(AL_ESLOT);
    let d = run_with(Some((3, AL_EALIGN))).map(|x| x.matched).ok() == Some(CORPUS as u32 - 1);
    a && b && c && d
}

/// 校验器。
fn checker_ok() -> bool {
    let mut c = Checker::new();
    let a = !c.bound() && c.steps() == 0;
    let b = match c.check(&Image::two(Seg::text(), Seg::rodata())) {
        Ok(v) => v == Fault::Ok && c.last() == 0,
        Err(_) => false,
    };
    let d = c.check(&Image::one(Seg::text())).err() == Some(AL_ESTATE);
    let e = c.bound() && c.steps() == 1 && c.last() == AL_ESTATE;
    a && b && d && e
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && arithmetic_ok();
    out[1] = seg_ok() && congruent_ok() && not2_ok();
    out[2] = cong_ok() && page_ok() && bias_ok() && over_ok();
    out[3] = bound_ok() && type_ok() && message_ok() && tail_message_ok()
        && corpus_ok() && slots_ok() && reds_ok() && checker_ok()
        && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11613_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11613.align_page");

    s.add(
        "AL01-名称与段基址",
        {
            name_ok()
        },
        "static",
    );

    s.add(
        "AL02-页算术与偏置",
        {
            arithmetic_ok()
        },
        "static",
    );

    s.add(
        "AL03-单段通过面",
        {
            seg_ok()
        },
        "static",
    );

    s.add(
        "AL04-幂与页小拒收",
        {
            not2_ok()
        },
        "static",
    );

    s.add(
        "AL05-同余闸门",
        {
            cong_ok()
        },
        "static",
    );

    s.add(
        "AL06-页偏闸门",
        {
            page_ok()
        },
        "static",
    );

    s.add(
        "AL07-统一偏置闸门",
        {
            bias_ok()
        },
        "static",
    );

    s.add(
        "AL08-重叠闸门",
        {
            over_ok()
        },
        "static",
    );

    s.add(
        "AL09-越界与上界",
        {
            bound_ok()
        },
        "static",
    );

    s.add(
        "AL10-类型与空图",
        {
            type_ok()
        },
        "static",
    );

    s.add(
        "AL11-文案逐字",
        {
            message_ok() && tail_message_ok()
        },
        "static",
    );

    s.add(
        "AL12-语料与汇总",
        {
            corpus_ok() && slots_ok() && reds_ok() && checker_ok()
        },
        "static",
    );

    s.add(
        "AL13-冻结重放四注入",
        {
            let mut l = build_ledger();
            let a = l.audit() == Ok(());
            let b = match l.freeze() {
                Ok(r) => r != 0 && l.is_frozen() && l.replay() == Ok(r),
                Err(_) => false,
            };
            let inj = inject_four();
            a && b && inj[0] && inj[1] && inj[2] && inj[3]
        },
        "static",
    );

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn fail() -> ! {
        panic!("unx11613 test fail")
    }

    #[test]
    fn pow2_table() {
        assert!(pow2(1));
        assert!(pow2(2));
        assert!(pow2(4));
        assert!(pow2(PAGE));
        assert!(!pow2(0));
        assert!(!pow2(3));
        assert!(!pow2(0x1800));
        assert!(!pow2(0x3000));
    }

    #[test]
    fn floor_ceil() {
        assert_eq!(page_floor(0), 0);
        assert_eq!(page_floor(0xfff), 0);
        assert_eq!(page_floor(PAGE), PAGE);
        assert_eq!(page_ceil(0), 0);
        assert_eq!(page_ceil(1), PAGE);
        assert_eq!(page_ceil(PAGE), PAGE);
        assert_eq!(page_ceil(PAGE + 1), 2 * PAGE);
    }

    #[test]
    fn congruence_table() {
        assert!(congruent(0x400000, 0, PAGE));
        assert!(!congruent(0x401800, 0, PAGE));
        assert!(congruent(7, 3, 2));
        assert!(congruent(7, 3, 0));
        assert!(congruent(7, 3, 1));
    }

    #[test]
    fn page_congruence_table() {
        assert!(page_congruent(0x401100, 0x100));
        assert!(!page_congruent(0x401000, 0x100));
        assert!(page_congruent(0x400ffe, 0xffe));
    }

    #[test]
    fn bias_math() {
        assert_eq!(bias_of(0x401000, 0x1000), 0x400000);
        assert_eq!(bias_of(0x402000, 0x2000), 0x400000);
        assert_eq!(bias_of(0x402000, 0x1000), 0x401000);
        assert_eq!(bias_of(0x400000, 0), 0x400000);
    }

    #[test]
    fn text_ok() {
        assert_eq!(validate(&Image::one(Seg::text())), Fault::Ok);
        assert_eq!(validate(&Image::one(Seg::rodata())), Fault::Ok);
        assert_eq!(validate(&Image::one(Seg::data())), Fault::Ok);
    }

    #[test]
    fn multi_ok() {
        let img = Image::three(Seg::text(), Seg::rodata(), Seg::data());
        assert_eq!(validate(&img), Fault::Ok);
        assert_eq!(img.unified_bias(), Some(0x400000));
        assert_eq!(img.len(), 3);
        assert!(img.get(3).is_none());
    }

    #[test]
    fn big_align_ok() {
        let s = Seg::new(PT_LOAD, 0, 0x400000, 0x100, 0x100, 0x20_0000);
        assert_eq!(validate(&Image::one(s)), Fault::Ok);
        assert_eq!(s.bias(), 0x400000);
    }

    #[test]
    fn align_zero_one_ok() {
        let z = Seg::new(PT_LOAD, 0x3000, 0x403000, 0x10, 0x10, 0);
        let o = Seg::new(PT_LOAD, 0x1234, 0x401234, 0x10, 0x10, 1);
        assert_eq!(validate(&Image::one(z)), Fault::Ok);
        assert_eq!(validate(&Image::one(o)), Fault::Ok);
    }

    #[test]
    fn not2_fault() {
        let s = Seg::new(PT_LOAD, 0, 0x400000, 0x10, 0x10, 0x1800);
        assert_eq!(validate(&Image::one(s)), Fault::Not2);
        assert_eq!(Fault::Not2.code(), AL_EALIGN);
    }

    #[test]
    fn subpage_fault() {
        let s = Seg::new(PT_LOAD, 0x800, 0x400800, 0x10, 0x10, 0x800);
        assert_eq!(validate(&Image::one(s)), Fault::Small);
        assert_eq!(Fault::Small.code(), AL_EALIGN);
    }

    #[test]
    fn congruence_fault() {
        let s = Seg::new(PT_LOAD, 0, 0x401800, 0x10, 0x10, PAGE);
        assert_eq!(validate(&Image::one(s)), Fault::Cong);
        assert_eq!(Fault::Cong.errno(), EINVAL);
    }

    #[test]
    fn page_skew_fault() {
        let s = Seg::new(PT_LOAD, 0x100, 0x401000, 0x10, 0x10, 1);
        assert_eq!(validate(&Image::one(s)), Fault::Skew);
        assert_eq!(Fault::Skew.errno(), EINVAL);
    }

    #[test]
    fn overlap_fault() {
        let img = Image::two(Seg::text(), Seg::new(PT_LOAD, 0x800, 0x400800, 0x10, 0x10, PAGE));
        assert_eq!(validate(&img), Fault::Over);
        assert_eq!(overlap_pair(&img), Some((0, 1)));
    }

    #[test]
    fn adjacent_pages_ok() {
        let img = Image::two(Seg::text(), Seg::new(PT_LOAD, 0x1000, 0x401000, 0x10, 0x10, PAGE));
        assert_eq!(validate(&img), Fault::Ok);
        assert!(overlap_pair(&img).is_none());
    }

    #[test]
    fn bias_fault() {
        let img = Image::two(Seg::text(), Seg::new(PT_LOAD, 0x1000, 0x402000, 0x10, 0x10, PAGE));
        assert_eq!(validate(&img), Fault::Bias);
        assert_eq!(bias_split(&img), Some((1, 0x401000, 0x400000)));
        assert_eq!(Fault::Bias.code(), AL_EBIAS);
    }

    #[test]
    fn bias_consistent_ok() {
        let img = Image::three(Seg::text(), Seg::rodata(), Seg::data());
        assert_eq!(bias_split(&img), None);
        assert_eq!(img.unified_bias(), Some(0x400000));
    }

    #[test]
    fn high_fault() {
        let s = Seg::new(PT_LOAD, 0, 0x8000_0000_0000, 0x10, 0x10, PAGE);
        assert_eq!(validate(&Image::one(s)), Fault::High);
        assert_eq!(Fault::High.code(), AL_EHIGH);
    }

    #[test]
    fn mem_overflow_fault() {
        let s = Seg::new(PT_LOAD, 0, 0x400000, 0x10, u64::MAX, PAGE);
        assert!(s.mem_end().is_none());
        assert_eq!(validate(&Image::one(s)), Fault::High);
    }

    #[test]
    fn type_fault() {
        let s = Seg::new(PT_DYNAMIC, 0x40, 0x400040, 0x10, 0x10, 1);
        assert_eq!(validate(&Image::one(s)), Fault::Type);
        assert_eq!(Fault::Type.errno(), ENOEXEC);
    }

    #[test]
    fn empty_image_fault() {
        assert_eq!(validate(&Image::new()), Fault::Empty);
        assert_eq!(Fault::Empty.code(), AL_ETYPE);
    }

    #[test]
    fn order_not2_before_cong() {
        let s = Seg::new(PT_LOAD, 1, 0x401001, 0x10, 0x10, 0x1800);
        assert_eq!(validate(&Image::one(s)), Fault::Not2);
    }

    #[test]
    fn order_seg_before_bias() {
        let img = Image::two(Seg::text(), Seg::new(PT_LOAD, 0x100, 0x401000, 0x10, 0x10, 1));
        assert_eq!(validate(&img), Fault::Skew);
    }

    #[test]
    fn message_verbatim() {
        let n2 = Image::one(Seg::new(PT_LOAD, 0, 0x400000, 0x10, 0x10, 0x1800));
        assert_eq!(
            message(Fault::Not2, &n2).as_str(),
            "./prog: bad PT_LOAD: p_align=0x1800 not a power of two\n"
        );
        let sm = Image::one(Seg::new(PT_LOAD, 0x800, 0x400800, 0x10, 0x10, 0x800));
        assert_eq!(
            message(Fault::Small, &sm).as_str(),
            "./prog: bad PT_LOAD: p_align=0x800 below page size 0x1000\n"
        );
        let cg = Image::one(Seg::new(PT_LOAD, 0, 0x401800, 0x10, 0x10, PAGE));
        assert_eq!(
            message(Fault::Cong, &cg).as_str(),
            "./prog: bad PT_LOAD: p_vaddr=0x401800 not congruent with p_offset=0x0 mod p_align=0x1000\n"
        );
    }

    #[test]
    fn message_tail() {
        let sk = Image::one(Seg::new(PT_LOAD, 0x100, 0x401000, 0x10, 0x10, 1));
        assert_eq!(
            message(Fault::Skew, &sk).as_str(),
            "./prog: bad PT_LOAD: p_vaddr=0x401000 not page-congruent with p_offset=0x100 (mod 0x1000)\n"
        );
        let ov = Image::two(Seg::text(), Seg::new(PT_LOAD, 0x800, 0x400800, 0x10, 0x10, PAGE));
        assert_eq!(
            message(Fault::Over, &ov).as_str(),
            "./prog: PT_LOAD segments overlap: [0x400000,0x401000) and [0x400000,0x401000)\n"
        );
        let bi = Image::two(Seg::text(), Seg::new(PT_LOAD, 0x1000, 0x402000, 0x10, 0x10, PAGE));
        assert_eq!(
            message(Fault::Bias, &bi).as_str(),
            "./prog: inconsistent load bias: segment 1 has 0x401000, first 0x400000\n"
        );
        assert_eq!(
            message(Fault::Ok, &Image::one(Seg::text())).as_str(),
            "ok: segments=1 bias=0x400000\n"
        );
        assert_eq!(
            message(Fault::Empty, &Image::new()).as_str(),
            "./prog: empty image: no loadable segments\n"
        );
    }

    #[test]
    fn corpus_shape() {
        assert_eq!(CASES.len(), CORPUS);
        assert!(unique());
        assert!(notes_ok());
        assert!(table_ok());
    }

    #[test]
    fn report_counts() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(r.total, 14);
        assert_eq!(r.accept, 8);
        assert_eq!(r.reject, 6);
        assert_eq!(r.not2, 1);
        assert_eq!(r.small, 1);
        assert_eq!(r.cong, 1);
        assert_eq!(r.skew, 1);
        assert_eq!(r.over, 1);
        assert_eq!(r.bias, 1);
        assert_eq!(r.matched, 14);
        assert_eq!(r.rate(), 1000);
        assert_eq!(r.gate(), Ok(()));
    }

    #[test]
    fn report_render_exact() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(
            r.render().as_str(),
            "align corpus=14 accept=8 reject=6 not2=1 small=1 cong=1 skew=1 over=1 bias=1 match=14 rate=1000ppm\n"
        );
    }

    #[test]
    fn red_override() {
        let r = match run_with(Some((0, AL_EALIGN))) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(r.matched, 13);
        assert_eq!(r.rate(), 929);
        assert_eq!(r.gate(), Err(AL_EVER));
        assert_eq!(run_with(Some((14, AL_EALIGN))).err(), Some(AL_ESLOT));
    }

    #[test]
    fn checker_once() {
        let mut c = Checker::new();
        match c.check(&Image::one(Seg::text())) {
            Ok(v) => assert_eq!(v, Fault::Ok),
            Err(_) => fail(),
        }
        assert_eq!(c.check(&Image::one(Seg::text())).err(), Some(AL_ESTATE));
        assert_eq!(c.steps(), 1);
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert!(l.sample(100).is_none());
        assert!(!l.is_empty());
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let r = match l.freeze() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_ne!(r, 0);
        assert!(l.is_frozen());
        assert_eq!(l.root(), r);
        assert_eq!(l.replay(), Ok(r));
        assert_eq!(l.register(Sample::blank()).err(), Some(AL_ESEAL));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.freeze().err(), Some(AL_ESEAL));
        assert_eq!(l.rejects(), 2);
    }

    #[test]
    fn injections() {
        let out = inject_four();
        assert!(out[0], "inject0");
        assert!(out[1], "inject1");
        assert!(out[2], "inject2");
        assert!(out[3], "inject3");
    }

    #[test]
    fn fold_and_text() {
        assert_eq!(Seg::text().seal(), Seg::text().seal());
        assert_ne!(Seg::text().seal(), Seg::rodata().seal());
        assert_ne!(Image::one(Seg::text()).seal(), Image::two(Seg::text(), Seg::rodata()).seal());
        assert_eq!(u32_text(0).as_str(), "0");
        assert_eq!(u32_text(14).as_str(), "14");
        assert_eq!(u32_text(929).as_str(), "929");
        assert_eq!(hex_text(0).as_str(), "0x0");
        assert_eq!(hex_text(0x1000).as_str(), "0x1000");
        assert_eq!(hex_text(0x7fff_ffff_f000).as_str(), "0x7ffffffff000");
    }

    #[test]
    fn slot_bounds() {
        assert!(slot_at(0).is_some());
        assert!(slot_at(CORPUS - 1).is_some());
        assert!(slot_at(CORPUS).is_none());
        assert_eq!(short(Fault::Ok), "ok");
        assert_eq!(short(Fault::Bias), "bias-split");
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11613_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }
}
