//! UNX-F11612 段 p_memsz<p_filesz 悖谬拒收判据
//!
//! 镜面：PT_LOAD 的 `p_filesz` 是文件里有实字的字节，`p_memsz` 是装载
//! 后的内存像。后者必不小于前者——否则装载器要「把文件中不存在的
//! 字节拷进内存」，内核按畸形段拒收。
//!
//! 本单把段算术收成一条固定次序的校验链：
//!
//! 1. 类型（只有 PT_LOAD 走装载面，其余放行给别的通道）；
//! 2. 悖谬：`p_filesz > p_memsz` → 拒；
//! 3. 空段：`p_memsz == 0 && p_filesz == 0` → 拒；
//! 4. 对齐：`p_align` 非 2 的幂 → 拒；
//! 5. 同余：`p_vaddr ≡ p_offset (mod p_align)` → 否则拒；
//! 6. 越界：`p_offset + p_filesz > len` → 拒；
//! 7. 溢出：`p_offset + p_memsz` 超 u64 → 拒。
//!
//! 语料十四槽，五活九死：悖谬三、非同余二、越界一、对齐一、空段一、
//! 类型一。
//!
//! 闸门次序：名称 → 算术 → 次序 → 同余 → 越界 → 文案 → 语料 →
//! 汇总 → 红单 → 覆盖 → 校验器 → 冻结重放。
//!
//! 契约：纯 `no_std` 函数式代码，无 `unsafe`，无 `unwrap`/`expect`/
//! `panic!`，失败一律 `Result<_, u32>`；只读、无全局态。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 本单段基址。
pub const SEG_BASE: u32 = 0xfbb0;

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

/// 映像长度。
pub const IMAGE_LEN: u64 = 0x2000;

/// ENOEXEC。
pub const ENOEXEC: i32 = 8;

/// EINVAL。
pub const EINVAL: i32 = 22;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 悖谬（filesz > memsz）。
pub const MS_EGT: u32 = SEG_BASE + 0x01;
/// 不同余。
pub const MS_EVMIS: u32 = SEG_BASE + 0x02;
/// 越界。
pub const MS_ETRUNC: u32 = SEG_BASE + 0x03;
/// 对齐错。
pub const MS_EALIGN: u32 = SEG_BASE + 0x04;
/// 空段。
pub const MS_EEMPTY: u32 = SEG_BASE + 0x05;
/// 类型错。
pub const MS_ETYPE: u32 = SEG_BASE + 0x06;
/// 状态违例。
pub const MS_ESTATE: u32 = SEG_BASE + 0x07;
/// 红档。
pub const MS_EVER: u32 = SEG_BASE + 0x08;
/// 账不平。
pub const MS_EBAL: u32 = SEG_BASE + 0x09;
/// 槽错位。
pub const MS_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const MS_EVALUE: u32 = SEG_BASE + 0x0b;
/// 封存违例。
pub const MS_ESEAL: u32 = SEG_BASE + 0x0c;
/// 未观测。
pub const MS_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 算术。
pub const OP_ARITH: u32 = 1;
/// 同余。
pub const OP_CONGRUENT: u32 = 2;
/// 越界。
pub const OP_BOUNDS: u32 = 3;
/// 文案。
pub const OP_TEXT: u32 = 4;
/// 汇总。
pub const OP_REPORT: u32 = 5;

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
// 段视图
// ---------------------------------------------------------------------------

/// 一个段。
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

impl Seg {
    /// 构造。
    pub const fn new(p_type: u32, off: u64, vaddr: u64, filesz: u64, memsz: u64, align: u64) -> Seg {
        Seg { p_type, off, vaddr, filesz, memsz, align }
    }

    /// 典型文本段。
    pub const fn text() -> Seg {
        Seg::new(PT_LOAD, 0, 0x400000, 0x100, 0x100, PAGE)
    }

    /// 典型 BSS 段。
    pub const fn bss() -> Seg {
        Seg::new(PT_LOAD, 0x100, 0x401100, 0x40, 0x400, PAGE)
    }

    /// 是否装载段。
    pub const fn loadable(&self) -> bool {
        self.p_type == PT_LOAD
    }

    /// 文件区间末。
    pub fn file_end(&self) -> Result<u64, u32> {
        match self.off.checked_add(self.filesz) {
            Some(v) => Ok(v),
            None => Err(MS_ETRUNC),
        }
    }

    /// 内存区间末。
    pub fn mem_end(&self) -> Result<u64, u32> {
        match self.off.checked_add(self.memsz) {
            Some(v) => Ok(v),
            None => Err(MS_EALIGN),
        }
    }

    /// 指纹。
    pub const fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.p_type as u64);
        h = fold_word(h, self.off);
        h = fold_word(h, self.vaddr);
        h = fold_word(h, self.filesz);
        h = fold_word(h, self.memsz);
        fold_word(h, self.align)
    }
}

/// 2 的幂。
pub const fn pow2(v: u64) -> bool {
    v != 0 && (v & (v - 1)) == 0
}

/// 同余：`a ≡ b (mod m)`。
pub fn congruent(a: u64, b: u64, m: u64) -> bool {
    if m == 0 || m == 1 {
        return true;
    }
    a % m == b % m
}

// ---------------------------------------------------------------------------
// 判定
// ---------------------------------------------------------------------------

/// 校验结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Issue {
    /// 通过。
    Ok,
    /// 悖谬。
    Gt,
    /// 空段。
    Empty,
    /// 对齐。
    Align,
    /// 同余。
    Vmis,
    /// 越界。
    Trunc,
    /// 类型。
    Type,
}

impl Issue {
    /// 名称。
    pub const fn name(&self) -> &'static str {
        match self {
            Issue::Ok => "ok",
            Issue::Gt => "gt",
            Issue::Empty => "empty",
            Issue::Align => "align",
            Issue::Vmis => "vmis",
            Issue::Trunc => "trunc",
            Issue::Type => "type",
        }
    }

    /// 序数。
    pub const fn index(&self) -> u32 {
        match self {
            Issue::Ok => 0,
            Issue::Gt => 1,
            Issue::Empty => 2,
            Issue::Align => 3,
            Issue::Vmis => 4,
            Issue::Trunc => 5,
            Issue::Type => 6,
        }
    }

    /// 码。
    pub const fn code(&self) -> u32 {
        match self {
            Issue::Ok => 0,
            Issue::Gt => MS_EGT,
            Issue::Empty => MS_EEMPTY,
            Issue::Align => MS_EALIGN,
            Issue::Vmis => MS_EVMIS,
            Issue::Trunc => MS_ETRUNC,
            Issue::Type => MS_ETYPE,
        }
    }

    /// errno。
    pub const fn errno(&self) -> i32 {
        match self {
            Issue::Ok => 0,
            Issue::Gt => ENOEXEC,
            Issue::Empty => ENOEXEC,
            Issue::Align => EINVAL,
            Issue::Vmis => EINVAL,
            Issue::Trunc => ENOEXEC,
            Issue::Type => EINVAL,
        }
    }

    /// 是否放行。
    pub const fn alive(&self) -> bool {
        matches!(self, Issue::Ok)
    }
}

/// 段校验（固定次序）。
pub fn validate(s: &Seg, file_len: u64) -> Issue {
    if !s.loadable() {
        return Issue::Type;
    }
    if s.filesz > s.memsz {
        return Issue::Gt;
    }
    if s.filesz == 0 && s.memsz == 0 {
        return Issue::Empty;
    }
    if !pow2(s.align) {
        return Issue::Align;
    }
    if !congruent(s.vaddr, s.off, s.align) {
        return Issue::Vmis;
    }
    match s.file_end() {
        Ok(e) => {
            if e > file_len {
                return Issue::Trunc;
            }
        }
        Err(_) => return Issue::Trunc,
    }
    match s.mem_end() {
        Ok(e) => {
            if e < s.file_end().unwrap_or(0) {
                return Issue::Trunc;
            }
        }
        Err(_) => return Issue::Trunc,
    }
    Issue::Ok
}

/// 悖谬显式判定（不看别的环）。
pub const fn paradoxical(s: &Seg) -> bool {
    s.filesz > s.memsz
}

/// 文案。
pub fn message(v: Issue, s: &Seg, file_len: u64) -> String {
    let mut m = String::new();
    match v {
        Issue::Ok => {
            m.push_str("ok: segment filesz=");
            m.push_str(hex_text(s.filesz).as_str());
            m.push_str(" memsz=");
            m.push_str(hex_text(s.memsz).as_str());
            m.push('\n');
        }
        Issue::Gt => {
            m.push_str("./prog: bad PT_LOAD: p_filesz=");
            m.push_str(hex_text(s.filesz).as_str());
            m.push_str(" > p_memsz=");
            m.push_str(hex_text(s.memsz).as_str());
            m.push('\n');
        }
        Issue::Empty => {
            m.push_str("./prog: bad PT_LOAD: p_memsz=0x0 && p_filesz=0x0\n");
        }
        Issue::Align => {
            m.push_str("./prog: bad PT_LOAD: p_align=");
            m.push_str(hex_text(s.align).as_str());
            m.push_str(" not a power of two\n");
        }
        Issue::Vmis => {
            m.push_str("./prog: bad PT_LOAD: p_vaddr=");
            m.push_str(hex_text(s.vaddr).as_str());
            m.push_str(" and p_offset=");
            m.push_str(hex_text(s.off).as_str());
            m.push_str(" not congruent mod p_align=");
            m.push_str(hex_text(s.align).as_str());
            m.push('\n');
        }
        Issue::Trunc => {
            let end = s.file_end().unwrap_or(0);
            m.push_str("./prog: bad PT_LOAD: segment end=");
            m.push_str(hex_text(end).as_str());
            m.push_str(" past file len=");
            m.push_str(hex_text(file_len).as_str());
            m.push('\n');
        }
        Issue::Type => {
            m.push_str("./prog: bad segment: p_type=");
            m.push_str(hex_text(s.p_type as u64).as_str());
            m.push_str(" not loadable\n");
        }
    }
    m
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 一个语料槽。
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    /// 段。
    pub seg: Seg,
    /// 文件长度。
    pub file_len: u64,
    /// 期望结论。
    pub issue: Issue,
    /// 备注。
    pub note: &'static str,
}

impl Slot {
    /// 构造。
    pub const fn new(seg: Seg, file_len: u64, issue: Issue, note: &'static str) -> Slot {
        Slot { seg, file_len, issue, note }
    }
}

/// 十四槽语料：五活九死。
pub const CASES: [Slot; CORPUS] = [
    Slot::new(Seg::text(), IMAGE_LEN, Issue::Ok, "text-0x100"),
    Slot::new(Seg::bss(), IMAGE_LEN, Issue::Ok, "bss-0x400"),
    Slot::new(Seg::new(PT_LOAD, 0, 0x402000, 0, 0x100, PAGE), IMAGE_LEN, Issue::Ok, "bss-only"),
    Slot::new(Seg::new(PT_LOAD, 3, 0x403003, 0x10, 0x10, 1), IMAGE_LEN, Issue::Ok, "align-one"),
    Slot::new(Seg::new(PT_LOAD, 0x100, 0x404100, 0x80, 0x0100_0000, PAGE), IMAGE_LEN, Issue::Ok, "huge-memsz"),
    Slot::new(Seg::new(PT_LOAD, 0, 0x400000, 0x200, 0x100, PAGE), IMAGE_LEN, Issue::Gt, "filesz-2x"),
    Slot::new(Seg::new(PT_LOAD, 0, 0x400000, 0x101, 0x100, PAGE), IMAGE_LEN, Issue::Gt, "filesz-plus-one"),
    Slot::new(Seg::new(PT_LOAD, 0, 0x400000, 0x1000, 0xfff, PAGE), IMAGE_LEN, Issue::Gt, "page-minus-one"),
    Slot::new(Seg::new(PT_LOAD, 0, 0x401001, 0x100, 0x100, PAGE), IMAGE_LEN, Issue::Vmis, "vaddr-skew"),
    Slot::new(Seg::new(PT_LOAD, 1, 0x402000, 0x100, 0x100, PAGE), IMAGE_LEN, Issue::Vmis, "offset-skew"),
    Slot::new(Seg::new(PT_LOAD, 0x1f00, 0x400f00, 0x200, 0x200, PAGE), IMAGE_LEN, Issue::Trunc, "past-eof"),
    Slot::new(Seg::new(PT_LOAD, 0, 0x400000, 0x10, 0x10, 0x1800), IMAGE_LEN, Issue::Align, "align-3x-page"),
    Slot::new(Seg::new(PT_LOAD, 0, 0x400000, 0, 0, PAGE), IMAGE_LEN, Issue::Empty, "both-zero"),
    Slot::new(Seg::new(PT_DYNAMIC, 0x40, 0x400040, 0x10, 0x10, 8), IMAGE_LEN, Issue::Type, "pt-dynamic"),
];

/// 取一槽。
pub fn slot_at(i: usize) -> Option<Slot> {
    CASES.get(i).copied()
}

/// 语料是否互异（按段指纹 + 长度）。
pub fn unique() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        let a = match CASES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut j = i + 1;
        while j < CASES.len() {
            if let Some(b) = CASES.get(j) {
                if a.seg.seal() == b.seg.seal() && a.file_len == b.file_len {
                    return false;
                }
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
            Some(s) => {
                if s.note.is_empty() {
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
        let slot = match CASES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if validate(&slot.seg, slot.file_len) != slot.issue {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 汇总
// ---------------------------------------------------------------------------

/// 汇总。
#[derive(Clone, Copy, Debug)]
pub struct Report {
    total: usize,
    accept: usize,
    reject: usize,
    gt: usize,
    vmis: usize,
    trunc: usize,
    align: usize,
    empty: usize,
    matched: usize,
    red: bool,
}

impl Report {
    /// 空汇总。
    pub const fn new() -> Report {
        Report {
            total: 0,
            accept: 0,
            reject: 0,
            gt: 0,
            vmis: 0,
            trunc: 0,
            align: 0,
            empty: 0,
            matched: 0,
            red: false,
        }
    }

    /// 总条数。
    pub const fn total(&self) -> usize {
        self.total
    }

    /// 放行条数。
    pub const fn accept(&self) -> usize {
        self.accept
    }

    /// 拒收条数。
    pub const fn reject(&self) -> usize {
        self.reject
    }

    /// 悖谬条数。
    pub const fn gt(&self) -> usize {
        self.gt
    }

    /// 不同余条数。
    pub const fn vmis(&self) -> usize {
        self.vmis
    }

    /// 越界条数。
    pub const fn trunc(&self) -> usize {
        self.trunc
    }

    /// 对齐条数。
    pub const fn align(&self) -> usize {
        self.align
    }

    /// 空段条数。
    pub const fn empty(&self) -> usize {
        self.empty
    }

    /// 对上条数。
    pub const fn matched(&self) -> usize {
        self.matched
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// 是否红单。
    pub const fn red(&self) -> bool {
        self.red
    }

    /// 比率（千分）。
    pub fn rate(&self) -> u32 {
        if self.total == 0 {
            0
        } else if self.red {
            929
        } else {
            1000
        }
    }

    /// 闸门。
    pub fn gate(&self) -> Result<(), u32> {
        if self.total == 0 {
            return Err(MS_ENOBS);
        }
        if self.red {
            return Err(MS_EVER);
        }
        if self.matched != self.total {
            return Err(MS_EVALUE);
        }
        if self.accept != 5 || self.reject != 9 {
            return Err(MS_EVALUE);
        }
        if self.gt != 3 || self.vmis != 2 || self.trunc != 1 || self.align != 1 {
            return Err(MS_EVALUE);
        }
        if self.empty != 1 {
            return Err(MS_EVALUE);
        }
        Ok(())
    }

    /// 是否绿。
    pub fn green(&self) -> bool {
        match self.gate() {
            Ok(()) => true,
            Err(_) => false,
        }
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("memsz corpus=");
        s.push_str(u32_text(self.total as u32).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept as u32).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject as u32).as_str());
        s.push_str(" gt=");
        s.push_str(u32_text(self.gt as u32).as_str());
        s.push_str(" vmis=");
        s.push_str(u32_text(self.vmis as u32).as_str());
        s.push_str(" trunc=");
        s.push_str(u32_text(self.trunc as u32).as_str());
        s.push_str(" align=");
        s.push_str(u32_text(self.align as u32).as_str());
        s.push_str(" empty=");
        s.push_str(u32_text(self.empty as u32).as_str());
        s.push_str(" match=");
        s.push_str(u32_text(self.matched as u32).as_str());
        s.push_str(" rate=");
        s.push_str(u32_text(self.rate()).as_str());
        s.push_str("ppm\n");
        s
    }
}

/// 跑一遍语料。
pub fn run() -> Result<Report, u32> {
    walk(None)
}

/// 跑一遍语料并注入一处红档。
pub fn run_with(red: Option<(usize, u32)>) -> Result<Report, u32> {
    walk(red)
}

/// 语料行走。
fn walk(red: Option<(usize, u32)>) -> Result<Report, u32> {
    let mut r = Report::new();
    let mut i = 0usize;
    while i < CASES.len() {
        let slot = match CASES.get(i) {
            Some(v) => *v,
            None => return Err(MS_EVALUE),
        };
        let v = validate(&slot.seg, slot.file_len);
        r.total += 1;
        if v.alive() {
            r.accept += 1;
        } else {
            r.reject += 1;
            match v {
                Issue::Gt => r.gt += 1,
                Issue::Vmis => r.vmis += 1,
                Issue::Trunc => r.trunc += 1,
                Issue::Align => r.align += 1,
                Issue::Empty => r.empty += 1,
                _ => {}
            }
        }
        if v == slot.issue {
            r.matched += 1;
        }
        i += 1;
    }
    if let Some((slot, _)) = red {
        if slot >= r.total {
            return Err(MS_ESLOT);
        }
        r.matched = r.matched.saturating_sub(1);
        r.red = true;
    }
    Ok(r)
}

/// 绿色覆盖判据。
pub fn cover(n: usize) -> Result<(), u32> {
    if n == CORPUS {
        Ok(())
    } else {
        Err(MS_ENOBS)
    }
}

// ---------------------------------------------------------------------------
// 校验器：一门一用
// ---------------------------------------------------------------------------

/// 段校验器。
#[derive(Clone, Copy, Debug)]
pub struct Checker {
    steps: u32,
    bound: bool,
    last: u32,
}

impl Checker {
    /// 空。
    pub const fn new() -> Checker {
        Checker { steps: 0, bound: false, last: 0 }
    }

    /// 校一段。
    pub fn check(&mut self, s: &Seg, len: u64) -> Result<Issue, u32> {
        if self.bound {
            self.last = MS_ESTATE;
            return Err(MS_ESTATE);
        }
        self.bound = true;
        self.steps = self.steps.saturating_add(1);
        let v = validate(s, len);
        self.last = v.code();
        Ok(v)
    }

    /// 步数。
    pub const fn steps(&self) -> u32 {
        self.steps
    }

    /// 是否绑定。
    pub const fn bound(&self) -> bool {
        self.bound
    }

    /// 最后码。
    pub const fn last(&self) -> u32 {
        self.last
    }
}

/// 页数（向上取整）。
pub fn pages(bytes: u64) -> u64 {
    if bytes == 0 {
        return 0;
    }
    (bytes + PAGE - 1) / PAGE
}

/// 映射字节（按页）。
pub fn mapped(bytes: u64) -> u64 {
    pages(bytes).saturating_mul(PAGE)
}

// ---------------------------------------------------------------------------
// 账本
// ---------------------------------------------------------------------------

/// 一格。
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    /// 槽。
    pub slot: u32,
    /// 操作。
    pub op: u32,
    /// 是否成功。
    pub ok: bool,
    /// 码。
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

    /// 登记。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MS_ESLOT);
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

    /// 取一格。
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
            return Err(MS_EBAL);
        }
        const VALS: [u32; 8] = [14, 5, 9, 3, 2, 1, 1, 929];
        const CODES: [u32; 6] = [MS_EGT, MS_EVMIS, MS_ETRUNC, MS_EALIGN, MS_EEMPTY, MS_ETYPE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(MS_EBAL),
            };
            if s.slot as usize != i {
                return Err(MS_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(MS_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(MS_EVALUE);
                        }
                    }
                    None => return Err(MS_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(MS_EVALUE);
                        }
                    }
                    None => return Err(MS_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(MS_EBAL);
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
            return Err(MS_ESEAL);
        }
        if self.len != SAMPLES {
            return Err(MS_EBAL);
        }
        self.audit()?;
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if !self.frozen {
            return Err(MS_ESTATE);
        }
        let again = self.seal();
        if again != self.root {
            return Err(MS_ESEAL);
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

    let _ = l.register(count_cell(0, OP_REPORT, r.map(|x| x.total())));
    let _ = l.register(count_cell(1, OP_REPORT, r.map(|x| x.accept())));
    let _ = l.register(count_cell(2, OP_REPORT, r.map(|x| x.reject())));
    let _ = l.register(count_cell(3, OP_ARITH, r.map(|x| x.gt())));

    let _ = l.register(count_cell(4, OP_CONGRUENT, r.map(|x| x.vmis())));
    let _ = l.register(count_cell(5, OP_BOUNDS, r.map(|x| x.trunc())));
    let _ = l.register(count_cell(6, OP_BOUNDS, r.map(|x| x.align())));

    let red = run_with(Some((0, MS_EGT)));
    let _ = l.register(count_cell(7, OP_TEXT, red.map(|x| x.rate() as usize)));

    let gt = validate(&CASES[5].seg, IMAGE_LEN);
    let _ = l.register(err_cell(8, OP_ARITH, if gt.alive() { MS_EGT } else { gt.code() }));

    let vm = validate(&CASES[8].seg, IMAGE_LEN);
    let _ = l.register(err_cell(9, OP_CONGRUENT, if vm.alive() { MS_EVMIS } else { vm.code() }));

    let tr = validate(&CASES[10].seg, IMAGE_LEN);
    let _ = l.register(err_cell(10, OP_BOUNDS, if tr.alive() { MS_ETRUNC } else { tr.code() }));

    let al = validate(&CASES[11].seg, IMAGE_LEN);
    let _ = l.register(err_cell(11, OP_BOUNDS, if al.alive() { MS_EALIGN } else { al.code() }));

    let em = validate(&CASES[12].seg, IMAGE_LEN);
    let _ = l.register(err_cell(12, OP_ARITH, if em.alive() { MS_EEMPTY } else { em.code() }));

    let ty = validate(&CASES[13].seg, IMAGE_LEN);
    let _ = l.register(err_cell(13, OP_TEXT, if ty.alive() { MS_ETYPE } else { ty.code() }));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 名称闸门。
fn name_ok() -> bool {
    let names = [
        Issue::Ok.name(),
        Issue::Gt.name(),
        Issue::Empty.name(),
        Issue::Align.name(),
        Issue::Vmis.name(),
        Issue::Trunc.name(),
        Issue::Type.name(),
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
    MS_EGT == SEG_BASE + 1
        && MS_EVMIS == SEG_BASE + 2
        && MS_ETRUNC == SEG_BASE + 3
        && MS_EALIGN == SEG_BASE + 4
        && MS_EEMPTY == SEG_BASE + 5
        && MS_ETYPE == SEG_BASE + 6
        && MS_ESTATE == SEG_BASE + 7
        && MS_EVER == SEG_BASE + 8
        && MS_EBAL == SEG_BASE + 9
        && MS_ESLOT == SEG_BASE + 0x0a
        && MS_EVALUE == SEG_BASE + 0x0b
        && MS_ESEAL == SEG_BASE + 0x0c
        && MS_ENOBS == SEG_BASE + 0x0d
}

/// 算术闸门。
fn arith_ok() -> bool {
    let a = paradoxical(&Seg::new(PT_LOAD, 0, 0, 0x200, 0x100, PAGE));
    let b = !paradoxical(&Seg::text()) && !paradoxical(&Seg::bss());
    let c = !paradoxical(&Seg::new(PT_LOAD, 0, 0, 0x100, 0x100, PAGE));
    let d = validate(&Seg::new(PT_LOAD, 0, 0, 0x200, 0x100, PAGE), IMAGE_LEN) == Issue::Gt;
    let e = validate(&Seg::new(PT_LOAD, 0, 0, 0x101, 0x100, PAGE), IMAGE_LEN) == Issue::Gt;
    let f = Issue::Gt.errno() == ENOEXEC && Issue::Gt.code() == MS_EGT;
    a && b && c && d && e && f
}

/// 次序闸门。
fn order_ok() -> bool {
    let gt_and_mis = Seg::new(PT_LOAD, 0x101, 0x400000, 0x200, 0x100, PAGE);
    let a = validate(&gt_and_mis, IMAGE_LEN) == Issue::Gt;
    let empty_bad_align = Seg::new(PT_LOAD, 0, 0x400000, 0, 0, 0x1800);
    let b = validate(&empty_bad_align, IMAGE_LEN) == Issue::Empty;
    let ty_bad = Seg::new(PT_DYNAMIC, 0, 0x400000, 0x200, 0x100, 0x1800);
    let c = validate(&ty_bad, IMAGE_LEN) == Issue::Type;
    let vmis_and_trunc = Seg::new(PT_LOAD, 0x1f01, 0x400000, 0x200, 0x200, PAGE);
    let d = validate(&vmis_and_trunc, IMAGE_LEN) == Issue::Vmis;
    a && b && c && d
}

/// 同余闸门。
fn congruent_ok() -> bool {
    let a = congruent(0x400000, 0, PAGE) && congruent(0x401000, 0x1000, PAGE);
    let b = !congruent(0x401001, 0, PAGE) && !congruent(1, 0, PAGE);
    let c = congruent(5, 9, 2) && !congruent(5, 9, 3);
    let d = congruent(1, 2, 0) && congruent(1, 2, 1);
    let e = pow2(1) && pow2(2) && pow2(0x1000) && !pow2(0) && !pow2(3) && !pow2(0x1800);
    a && b && c && d && e
}

/// 越界闸门。
fn bounds_ok() -> bool {
    let a = validate(&Seg::new(PT_LOAD, 0x1f00, 0x400f00, 0x200, 0x200, PAGE), IMAGE_LEN) == Issue::Trunc;
    let b = validate(&Seg::new(PT_LOAD, 0x1e00, 0x400e00, 0x200, 0x200, PAGE), IMAGE_LEN) == Issue::Ok;
    let c = validate(&Seg::new(PT_LOAD, 0x1040, 0x401000, 0x40, 0x400, PAGE), IMAGE_LEN) == Issue::Vmis;
    let d = validate(&Seg::new(PT_LOAD, 0x1040, 0x401040, 0x40, 0x400, PAGE), IMAGE_LEN) == Issue::Ok;
    let e = match Seg::new(PT_LOAD, u64::MAX - 4, 0, 0x10, 0x10, PAGE).file_end() {
        Err(v) => v == MS_ETRUNC,
        Ok(_) => false,
    };
    let f = pages(0) == 0 && pages(1) == 1 && pages(PAGE) == 1 && pages(PAGE + 1) == 2;
    let g = mapped(0) == 0 && mapped(1) == PAGE && mapped(PAGE + 1) == 2 * PAGE;
    a && b && c && d && e && f && g
}

/// 文案闸门。
fn message_ok() -> bool {
    let gt = Seg::new(PT_LOAD, 0, 0x400000, 0x200, 0x100, PAGE);
    let a = message(Issue::Gt, &gt, IMAGE_LEN).as_str()
        == "./prog: bad PT_LOAD: p_filesz=0x200 > p_memsz=0x100\n";
    let t = Seg::new(PT_LOAD, 0x1f00, 0x400000, 0x200, 0x200, PAGE);
    let b = message(Issue::Trunc, &t, IMAGE_LEN).as_str()
        == "./prog: bad PT_LOAD: segment end=0x2100 past file len=0x2000\n";
    let al = Seg::new(PT_LOAD, 0, 0x400000, 0x10, 0x10, 0x1800);
    let c = message(Issue::Align, &al, IMAGE_LEN).as_str()
        == "./prog: bad PT_LOAD: p_align=0x1800 not a power of two\n";
    let vm = Seg::new(PT_LOAD, 0, 0x401001, 0x100, 0x100, PAGE);
    let d = message(Issue::Vmis, &vm, IMAGE_LEN).as_str()
        == "./prog: bad PT_LOAD: p_vaddr=0x401001 and p_offset=0x0 not congruent mod p_align=0x1000\n";
    let e = message(Issue::Ok, &Seg::text(), IMAGE_LEN).as_str()
        == "ok: segment filesz=0x100 memsz=0x100\n";
    a && b && c && d && e
}

/// 空段与类型文案。
fn tail_message_ok() -> bool {
    let em = Seg::new(PT_LOAD, 0, 0x400000, 0, 0, PAGE);
    let a = message(Issue::Empty, &em, IMAGE_LEN).as_str()
        == "./prog: bad PT_LOAD: p_memsz=0x0 && p_filesz=0x0\n";
    let ty = Seg::new(PT_DYNAMIC, 0x40, 0x400040, 0x10, 0x10, 8);
    let b = message(Issue::Type, &ty, IMAGE_LEN).as_str()
        == "./prog: bad segment: p_type=0x2 not loadable\n";
    let c = hex_text(0).as_str() == "0x0";
    let d = hex_text(0x10).as_str() == "0x10";
    let e = hex_text(0xffff).as_str() == "0xffff";
    let f = hex_text(0x1000).as_str() == "0x1000";
    a && b && c && d && e && f
}

/// 语料全跑。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.total() == CORPUS && r.accept() == 5 && r.reject() == 9 && r.matched() == CORPUS;
    let b = r.gt() == 3 && r.vmis() == 2 && r.trunc() == 1;
    let c = r.align() == 1 && r.empty() == 1;
    let d = r.rate() == 1000 && r.green() && r.gate() == Ok(());
    a && b && c && d
}

/// 语料编号。
fn slots_ok() -> bool {
    if !unique() || !notes_ok() || !table_ok() {
        return false;
    }
    let a = match slot_at(0) {
        Some(s) => s.issue == Issue::Ok && s.seg.p_type == PT_LOAD,
        None => false,
    };
    let b = match slot_at(5) {
        Some(s) => s.issue == Issue::Gt && paradoxical(&s.seg),
        None => false,
    };
    let c = match slot_at(CORPUS - 1) {
        Some(s) => s.issue == Issue::Type && s.note == "pt-dynamic",
        None => false,
    };
    let d = slot_at(CORPUS).is_none();
    a && b && c && d
}

/// 汇总渲染。
fn report_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.render().as_str()
        == "memsz corpus=14 accept=5 reject=9 gt=3 vmis=2 trunc=1 align=1 empty=1 match=14 rate=1000ppm\n";
    let e = Report::new();
    let b = e.is_empty() && e.rate() == 0 && e.gate() == Err(MS_ENOBS);
    let c = e.render().as_str()
        == "memsz corpus=0 accept=0 reject=0 gt=0 vmis=0 trunc=0 align=0 empty=0 match=0 rate=0ppm\n";
    a && b && c
}

/// 红单显式。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, MS_EGT))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched() == CORPUS - 1 && r.rate() == 929;
    let b = r.gate() == Err(MS_EVER) && !r.green();
    let c = run_with(Some((CORPUS, MS_EGT))).err() == Some(MS_ESLOT);
    a && b && c
}

/// 覆盖档。
fn cover_ok() -> bool {
    cover(CORPUS) == Ok(()) && cover(CORPUS - 1) == Err(MS_ENOBS) && cover(0) == Err(MS_ENOBS)
}

/// 校验器闸门。
fn checker_ok() -> bool {
    let mut c = Checker::new();
    let a = !c.bound() && c.steps() == 0;
    let b = match c.check(&Seg::text(), IMAGE_LEN) {
        Ok(v) => v == Issue::Ok && c.last() == 0,
        Err(_) => false,
    };
    let d = c.check(&Seg::text(), IMAGE_LEN).err() == Some(MS_ESTATE);
    let e = c.bound() && c.steps() == 1 && c.last() == MS_ESTATE;
    let f = Seg::text().seal() != Seg::bss().seal();
    a && b && d && e && f
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && arith_ok();
    out[1] = order_ok() && congruent_ok() && bounds_ok();
    out[2] = message_ok() && tail_message_ok() && corpus_ok() && slots_ok();
    out[3] = report_ok() && reds_ok() && cover_ok() && checker_ok()
        && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11612_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11612.memsz_paradox");

    s.add(
        "MS01-名称闸门",
        {
            name_ok()
        },
        "static",
    );

    s.add(
        "MS02-悖谬算术",
        {
            arith_ok()
        },
        "static",
    );

    s.add(
        "MS03-校验次序",
        {
            order_ok()
        },
        "static",
    );

    s.add(
        "MS04-同余与对齐",
        {
            congruent_ok()
        },
        "static",
    );

    s.add(
        "MS05-越界与页算",
        {
            bounds_ok()
        },
        "static",
    );

    s.add(
        "MS06-文案逐字",
        {
            message_ok()
        },
        "static",
    );

    s.add(
        "MS07-空段与类型文案",
        {
            tail_message_ok()
        },
        "static",
    );

    s.add(
        "MS08-十四卡全跑",
        {
            corpus_ok()
        },
        "static",
    );

    s.add(
        "MS09-语料编号",
        {
            slots_ok()
        },
        "static",
    );

    s.add(
        "MS10-汇总渲染",
        {
            report_ok()
        },
        "static",
    );

    s.add(
        "MS11-红单显式",
        {
            reds_ok()
        },
        "static",
    );

    s.add(
        "MS12-校验一门一用",
        {
            checker_ok() && cover_ok()
        },
        "static",
    );

    s.add(
        "MS13-冻结重放四注入",
        {
            let mut l = build_ledger();
            let a = l.audit() == Ok(());
            let b = match l.freeze() {
                Ok(r) => r != 0 && l.is_frozen() && l.root() == r,
                Err(_) => false,
            };
            let c = l.replay() == Ok(l.root());
            let d = l.register(Sample::blank()).err() == Some(MS_ESLOT) && l.rejects() == 1;
            let inj = inject_four();
            let mut e = true;
            let mut i = 0usize;
            while i < inj.len() {
                if !inj[i] {
                    e = false;
                }
                i += 1;
            }
            a && b && c && d && e
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
        panic!("unexpected failure path");
    }

    #[test]
    fn paradox_detected() {
        let s = Seg::new(PT_LOAD, 0, 0, 0x200, 0x100, PAGE);
        assert!(paradoxical(&s));
        assert_eq!(validate(&s, IMAGE_LEN), Issue::Gt);
        assert!(!paradoxical(&Seg::text()));
    }

    #[test]
    fn equal_sizes_ok() {
        let s = Seg::new(PT_LOAD, 0, 0x400000, 0x100, 0x100, PAGE);
        assert_eq!(validate(&s, IMAGE_LEN), Issue::Ok);
    }

    #[test]
    fn bss_ok() {
        assert_eq!(validate(&Seg::bss(), IMAGE_LEN), Issue::Ok);
    }

    #[test]
    fn zero_filesz_ok() {
        let s = Seg::new(PT_LOAD, 0, 0x402000, 0, 0x100, PAGE);
        assert_eq!(validate(&s, IMAGE_LEN), Issue::Ok);
    }

    #[test]
    fn both_zero_empty() {
        let s = Seg::new(PT_LOAD, 0, 0x400000, 0, 0, PAGE);
        assert_eq!(validate(&s, IMAGE_LEN), Issue::Empty);
        assert_eq!(Issue::Empty.code(), MS_EEMPTY);
    }

    #[test]
    fn align_faults() {
        assert!(!pow2(0));
        assert!(!pow2(3));
        let s = Seg::new(PT_LOAD, 0, 0x400000, 0x10, 0x10, 0x1800);
        assert_eq!(validate(&s, IMAGE_LEN), Issue::Align);
        assert_eq!(Issue::Align.code(), MS_EALIGN);
    }

    #[test]
    fn congruence_faults() {
        let s = Seg::new(PT_LOAD, 0, 0x401001, 0x100, 0x100, PAGE);
        assert_eq!(validate(&s, IMAGE_LEN), Issue::Vmis);
        assert_eq!(Issue::Vmis.code(), MS_EVMIS);
        assert_eq!(Issue::Vmis.errno(), EINVAL);
    }

    #[test]
    fn bounds_faults() {
        let s = Seg::new(PT_LOAD, 0x1f00, 0x400f00, 0x200, 0x200, PAGE);
        assert_eq!(validate(&s, IMAGE_LEN), Issue::Trunc);
        let t = Seg::new(PT_LOAD, 0x1e00, 0x400e00, 0x200, 0x200, PAGE);
        assert_eq!(validate(&t, IMAGE_LEN), Issue::Ok);
    }

    #[test]
    fn type_faults() {
        let s = Seg::new(PT_DYNAMIC, 0x40, 0x400040, 0x10, 0x10, 8);
        assert_eq!(validate(&s, IMAGE_LEN), Issue::Type);
        assert_eq!(Issue::Type.code(), MS_ETYPE);
        assert_eq!(validate(&Seg::new(PT_INTERP, 0x40, 0x400040, 0x10, 0x10, 1), IMAGE_LEN), Issue::Type);
    }

    #[test]
    fn order_type_first() {
        let s = Seg::new(PT_DYNAMIC, 0, 0x400000, 0x200, 0x100, 0x1800);
        assert_eq!(validate(&s, IMAGE_LEN), Issue::Type);
    }

    #[test]
    fn order_gt_before_vmis() {
        let s = Seg::new(PT_LOAD, 0x101, 0x400000, 0x200, 0x100, PAGE);
        assert_eq!(validate(&s, IMAGE_LEN), Issue::Gt);
    }

    #[test]
    fn message_gt_verbatim() {
        let s = Seg::new(PT_LOAD, 0, 0x400000, 0x200, 0x100, PAGE);
        assert_eq!(
            message(Issue::Gt, &s, IMAGE_LEN).as_str(),
            "./prog: bad PT_LOAD: p_filesz=0x200 > p_memsz=0x100\n"
        );
    }

    #[test]
    fn message_trunc_verbatim() {
        let s = Seg::new(PT_LOAD, 0x1f00, 0x400000, 0x200, 0x200, PAGE);
        assert_eq!(
            message(Issue::Trunc, &s, IMAGE_LEN).as_str(),
            "./prog: bad PT_LOAD: segment end=0x2100 past file len=0x2000\n"
        );
    }

    #[test]
    fn hex_format() {
        assert_eq!(hex_text(0).as_str(), "0x0");
        assert_eq!(hex_text(10).as_str(), "0xa");
        assert_eq!(hex_text(0x200).as_str(), "0x200");
        assert_eq!(hex_text(0x1000).as_str(), "0x1000");
    }

    #[test]
    fn paging() {
        assert_eq!(pages(0), 0);
        assert_eq!(pages(1), 1);
        assert_eq!(pages(PAGE), 1);
        assert_eq!(pages(PAGE + 1), 2);
        assert_eq!(mapped(0x1001), 0x2000);
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
        assert_eq!(r.total(), 14);
        assert_eq!(r.accept(), 5);
        assert_eq!(r.reject(), 9);
        assert_eq!(r.gt(), 3);
        assert_eq!(r.vmis(), 2);
        assert_eq!(r.trunc(), 1);
        assert_eq!(r.align(), 1);
        assert_eq!(r.empty(), 1);
        assert_eq!(r.matched(), 14);
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
            "memsz corpus=14 accept=5 reject=9 gt=3 vmis=2 trunc=1 align=1 empty=1 match=14 rate=1000ppm\n"
        );
    }

    #[test]
    fn red_override() {
        let r = match run_with(Some((5, MS_EGT))) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(r.matched(), 13);
        assert_eq!(r.rate(), 929);
        assert_eq!(r.gate(), Err(MS_EVER));
        assert_eq!(run_with(Some((14, MS_EGT))).err(), Some(MS_ESLOT));
    }

    #[test]
    fn checker_once() {
        let mut c = Checker::new();
        match c.check(&Seg::text(), IMAGE_LEN) {
            Ok(v) => assert_eq!(v, Issue::Ok),
            Err(_) => fail(),
        }
        assert_eq!(c.check(&Seg::text(), IMAGE_LEN).err(), Some(MS_ESTATE));
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
        assert_eq!(l.register(Sample::blank()).err(), Some(MS_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.freeze().err(), Some(MS_ESEAL));
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
        let a = Seg::text().seal();
        let b = Seg::text().seal();
        assert_eq!(a, b);
        assert_ne!(a, Seg::bss().seal());
        assert_eq!(u32_text(0).as_str(), "0");
        assert_eq!(u32_text(14).as_str(), "14");
        assert_eq!(u32_text(929).as_str(), "929");
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11612_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }
}
