//! UNX-F11615 正确 ELF 装载正判据（对照面锚）
//!
//! 镜面：前十四单判的是「什么样的坏件必须被拒」。本单反向锚定「好件装
//! 载后应当得到什么」——把装载结果收成一份可复算的计划面：
//!
//! 1. 装入偏置 `bias = floor_pg(p_vaddr) - floor_pg(p_offset)`，全部段一致；
//! 2. 入口 `e_entry + bias` 必须落在某个可执行（X）且非空的段内；
//! 3. `brk` 取全部段页末的最大值，任何段末溢出 u64 都使计划不可成立；
//! 4. 映像起点取全部段页首的最小值，页数取 `(brk - start) / PAGE`；
//! 5. 程序头在内存中的位置 `phoff + bias` 必须落在某个已映射段内；
//! 6. 段数为零、非 PT_LOAD 或空内存段使计划不成立。
//!
//! 计划面与「锚」逐字段比对：偏置、入口、brk、起点、页数、程序头地址、
//! 段数七项任一漂移都显式报错，绝不以近似值放行。
//!
//! 语料十四槽，八活六死——结构一、偏置一、入口二、brk 一、程序头一。
//!
//! 次序要害：结构先于偏置（没有可装载段就谈不上偏置）；偏置先于入口
//! （入口的地址解释依赖偏置）；入口先于 brk（入口是「能不能开始跑」，
//! brk 是「跑起来之后堆从哪里长」）；brk 先于程序头（程序头是辅助面，
//! 前四项都成立才有意义）。
//!
//! 闸门次序：名称 → 页算术与映射 → 正件计划 → 单段与 PIE → 四段 →
//! 偏置 → 入口 → brk → 程序头 → 结构 → 文案 → 语料汇总 → 冻结重放。
//!
//! 契约：纯 `no_std` 函数式代码，无 `unsafe`，无 `unwrap`/`expect`/
//! `panic!`，失败一律 `Result<_, u32>`；只读、无全局态。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 本单段基址。
pub const SEG_BASE: u32 = 0xfbe0;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// PT_LOAD。
pub const PT_LOAD: u32 = 1;

/// PT_DYNAMIC。
pub const PT_DYNAMIC: u32 = 2;

/// 页字节。
pub const PAGE: u64 = 0x1000;

/// 页内掩码。
pub const PMASK: u64 = PAGE - 1;

/// X 权限位。
pub const PF_X: u32 = 1;

/// W 权限位。
pub const PF_W: u32 = 2;

/// R 权限位。
pub const PF_R: u32 = 4;

/// 文本段权限。
pub const PF_RX: u32 = PF_R | PF_X;

/// 只读段权限。
pub const PF_RO: u32 = PF_R;

/// 数据段权限。
pub const PF_RW: u32 = PF_R | PF_W;

/// 映像最大段数。
pub const MAX_SEGS: usize = 4;

/// ENOEXEC。
pub const ENOEXEC: i32 = 8;

/// EINVAL。
pub const EINVAL: i32 = 22;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 段结构不成立。
pub const LD_ESEG: u32 = SEG_BASE + 0x01;
/// 偏置分裂。
pub const LD_EBASE: u32 = SEG_BASE + 0x02;
/// 入口不在可执行段。
pub const LD_EENTRY: u32 = SEG_BASE + 0x03;
/// brk 溢出。
pub const LD_EBRK: u32 = SEG_BASE + 0x04;
/// 程序头不在映射内。
pub const LD_EPHDR: u32 = SEG_BASE + 0x05;
/// 计划漂移。
pub const LD_EDRIFT: u32 = SEG_BASE + 0x06;
/// 状态违例。
pub const LD_ESTATE: u32 = SEG_BASE + 0x07;
/// 红档。
pub const LD_EVER: u32 = SEG_BASE + 0x08;
/// 账不平。
pub const LD_EBAL: u32 = SEG_BASE + 0x09;
/// 槽错位。
pub const LD_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const LD_EVALUE: u32 = SEG_BASE + 0x0b;
/// 封存违例。
pub const LD_ESEAL: u32 = SEG_BASE + 0x0c;
/// 未观测。
pub const LD_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 页算术。
pub const OP_PAGE: u32 = 1;
/// 偏置。
pub const OP_BIAS: u32 = 2;
/// 入口。
pub const OP_ENTRY: u32 = 3;
/// brk。
pub const OP_BRK: u32 = 4;
/// 程序头。
pub const OP_PHDR: u32 = 5;
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
        match lo.checked_add(PAGE) {
            Some(x) => x,
            None => u64::MAX,
        }
    }
}

/// 段偏置。
pub const fn bias_of(vaddr: u64, off: u64) -> u64 {
    page_floor(vaddr).wrapping_sub(page_floor(off))
}

// ---------------------------------------------------------------------------
// 段与映像
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
    /// 权限位。
    pub flags: u32,
    /// 对齐。
    pub align: u64,
}

impl Seg {
    /// 构造。
    pub const fn new(
        p_type: u32,
        off: u64,
        vaddr: u64,
        filesz: u64,
        memsz: u64,
        flags: u32,
        align: u64,
    ) -> Seg {
        Seg { p_type, off, vaddr, filesz, memsz, flags, align }
    }

    /// 文本段。
    pub const fn text() -> Seg {
        Seg::new(PT_LOAD, 0, 0x400000, 0x100, 0x100, PF_RX, PAGE)
    }

    /// 只读段。
    pub const fn rodata() -> Seg {
        Seg::new(PT_LOAD, 0x1000, 0x401000, 0x100, 0x100, PF_RO, PAGE)
    }

    /// 数据段。
    pub const fn data() -> Seg {
        Seg::new(PT_LOAD, 0x2000, 0x402000, 0x100, 0x100, PF_RW, PAGE)
    }

    /// 可执行。
    pub const fn executable(&self) -> bool {
        self.flags & PF_X != 0
    }

    /// 是否装载段。
    pub const fn loadable(&self) -> bool {
        self.p_type == PT_LOAD
    }

    /// 段末。
    pub const fn mem_end(&self) -> Option<u64> {
        self.vaddr.checked_add(self.memsz)
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
        h = fold_word(h, self.flags as u64);
        h = fold_word(h, self.align);
        h
    }
}

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
        Image { segs: [Seg::new(0, 0, 0, 0, 0, 0, 1); MAX_SEGS], count: 0 }
    }

    /// 一段图。
    pub const fn one(a: Seg) -> Image {
        let mut segs = [Seg::new(0, 0, 0, 0, 0, 0, 1); MAX_SEGS];
        segs[0] = a;
        Image { segs, count: 1 }
    }

    /// 两段图。
    pub const fn two(a: Seg, b: Seg) -> Image {
        let mut segs = [Seg::new(0, 0, 0, 0, 0, 0, 1); MAX_SEGS];
        segs[0] = a;
        segs[1] = b;
        Image { segs, count: 2 }
    }

    /// 三段图。
    pub const fn three(a: Seg, b: Seg, c: Seg) -> Image {
        let mut segs = [Seg::new(0, 0, 0, 0, 0, 0, 1); MAX_SEGS];
        segs[0] = a;
        segs[1] = b;
        segs[2] = c;
        Image { segs, count: 3 }
    }

    /// 四段图。
    pub const fn four(a: Seg, b: Seg, c: Seg, d: Seg) -> Image {
        let mut segs = [Seg::new(0, 0, 0, 0, 0, 0, 1); MAX_SEGS];
        segs[0] = a;
        segs[1] = b;
        segs[2] = c;
        segs[3] = d;
        Image { segs, count: 4 }
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

    /// 统一偏置。
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

/// 查地址命中的段。
pub fn mapping_at(img: &Image, addr: u64) -> Option<Seg> {
    let mut i = 0usize;
    while i < img.len() {
        let s = match img.get(i) {
            Some(v) => *v,
            None => return None,
        };
        let end = match s.mem_end() {
            Some(v) => v,
            None => {
                i += 1;
                continue;
            }
        };
        if addr >= s.vaddr && addr < end {
            return Some(s);
        }
        i += 1;
    }
    None
}

/// 查地址命中的可执行段。
pub fn exec_at(img: &Image, addr: u64) -> Option<Seg> {
    match mapping_at(img, addr) {
        Some(s) => {
            if s.executable() {
                Some(s)
            } else {
                None
            }
        }
        None => None,
    }
}

// ---------------------------------------------------------------------------
// 计划
// ---------------------------------------------------------------------------

/// 判面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 通过。
    Ok,
    /// 段结构不成立。
    Seg,
    /// 偏置分裂。
    Base,
    /// 入口错。
    Entry,
    /// brk 溢出。
    Brk,
    /// 程序头不在映射内。
    Phdr,
    /// 计划漂移。
    Drift,
}

impl Fault {
    /// 名称。
    pub const fn name(&self) -> &'static str {
        match self {
            Fault::Ok => "ok",
            Fault::Seg => "segment",
            Fault::Base => "bias",
            Fault::Entry => "entry",
            Fault::Brk => "brk",
            Fault::Phdr => "phdr",
            Fault::Drift => "drift",
        }
    }

    /// 错误码。
    pub const fn code(&self) -> u32 {
        match self {
            Fault::Ok => 0,
            Fault::Seg => LD_ESEG,
            Fault::Base => LD_EBASE,
            Fault::Entry => LD_EENTRY,
            Fault::Brk => LD_EBRK,
            Fault::Phdr => LD_EPHDR,
            Fault::Drift => LD_EDRIFT,
        }
    }

    /// errno。
    pub const fn errno(&self) -> i32 {
        match self {
            Fault::Ok => 0,
            Fault::Seg | Fault::Base => ENOEXEC,
            _ => EINVAL,
        }
    }

    /// 是否拒收面。
    pub const fn alive(&self) -> bool {
        !matches!(self, Fault::Ok)
    }
}

/// 装载计划面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    /// 装入偏置。
    pub bias: u64,
    /// 实际入口。
    pub entry: u64,
    /// 堆底。
    pub brk: u64,
    /// 映像页首。
    pub start: u64,
    /// 映像页数。
    pub pages: u64,
    /// 程序头内存地址。
    pub phdr: u64,
    /// 段数。
    pub segs: u32,
}

impl Plan {
    /// 锚：典型三段的正确计划。
    pub const fn anchor() -> Plan {
        Plan { bias: 0x400000, entry: 0x400080, brk: 0x403000, start: 0x400000, pages: 3, phdr: 0x400040, segs: 3 }
    }

    /// 单段锚。
    pub const fn single() -> Plan {
        Plan { bias: 0x400000, entry: 0x400080, brk: 0x401000, start: 0x400000, pages: 1, phdr: 0x400040, segs: 1 }
    }

    /// PIE 锚。
    pub const fn pie() -> Plan {
        Plan { bias: 0, entry: 0x80, brk: 0x2000, start: 0, pages: 2, phdr: 0x40, segs: 2 }
    }

    /// 逐字段相等（不含派生指纹）。
    pub const fn matches(&self, other: &Plan) -> bool {
        self.bias == other.bias
            && self.entry == other.entry
            && self.brk == other.brk
            && self.start == other.start
            && self.pages == other.pages
            && self.phdr == other.phdr
            && self.segs == other.segs
    }

    /// 首个漂移字段序号；无漂移为 `None`。
    pub const fn drift(&self, want: &Plan) -> Option<u32> {
        if self.bias != want.bias {
            return Some(0);
        }
        if self.entry != want.entry {
            return Some(1);
        }
        if self.brk != want.brk {
            return Some(2);
        }
        if self.start != want.start {
            return Some(3);
        }
        if self.pages != want.pages {
            return Some(4);
        }
        if self.phdr != want.phdr {
            return Some(5);
        }
        if self.segs != want.segs {
            return Some(6);
        }
        None
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.bias);
        h = fold_word(h, self.entry);
        h = fold_word(h, self.brk);
        h = fold_word(h, self.start);
        h = fold_word(h, self.pages);
        h = fold_word(h, self.phdr);
        h = fold_word(h, self.segs as u64);
        h
    }
}

/// 计算装载计划。
pub fn plan(img: &Image, phoff: u64, e_entry: u64) -> Result<Plan, Fault> {
    if img.is_empty() {
        return Err(Fault::Seg);
    }
    let mut i = 0usize;
    while i < img.len() {
        let s = match img.get(i) {
            Some(v) => *v,
            None => return Err(Fault::Seg),
        };
        if !s.loadable() || s.memsz == 0 {
            return Err(Fault::Seg);
        }
        i += 1;
    }
    let bias = match img.unified_bias() {
        Some(v) => v,
        None => return Err(Fault::Base),
    };
    let entry = match e_entry.checked_add(bias) {
        Some(v) => v,
        None => return Err(Fault::Entry),
    };
    if exec_at(img, entry).is_none() {
        return Err(Fault::Entry);
    }
    let mut brk = 0u64;
    let mut start = u64::MAX;
    let mut j = 0usize;
    while j < img.len() {
        let s = match img.get(j) {
            Some(v) => *v,
            None => return Err(Fault::Seg),
        };
        let end = match s.mem_end() {
            Some(v) => v,
            None => return Err(Fault::Brk),
        };
        let hi = match end.checked_add(PMASK) {
            Some(v) => page_floor(v),
            None => return Err(Fault::Brk),
        };
        if hi > brk {
            brk = hi;
        }
        let lo = page_floor(s.vaddr);
        if lo < start {
            start = lo;
        }
        j += 1;
    }
    if brk < start {
        return Err(Fault::Brk);
    }
    let pages = (brk - start) / PAGE;
    let phdr = match phoff.checked_add(bias) {
        Some(v) => v,
        None => return Err(Fault::Phdr),
    };
    if mapping_at(img, phdr).is_none() {
        return Err(Fault::Phdr);
    }
    Ok(Plan { bias, entry, brk, start, pages, phdr, segs: img.len() as u32 })
}

/// 对照锚校验。
pub fn compare(p: &Plan, want: &Plan) -> Result<(), u32> {
    if p.matches(want) {
        Ok(())
    } else {
        Err(LD_EDRIFT)
    }
}

/// 漂移文案。
pub fn drift_message(p: &Plan, want: &Plan) -> String {
    let mut s = String::from("./prog: ELF load plan drift: ");
    match p.drift(want) {
        Some(0) => {
            s.push_str("bias=");
            s.push_str(&hex_text(p.bias));
            s.push_str(" want ");
            s.push_str(&hex_text(want.bias));
        }
        Some(1) => {
            s.push_str("entry=");
            s.push_str(&hex_text(p.entry));
            s.push_str(" want ");
            s.push_str(&hex_text(want.entry));
        }
        Some(2) => {
            s.push_str("brk=");
            s.push_str(&hex_text(p.brk));
            s.push_str(" want ");
            s.push_str(&hex_text(want.brk));
        }
        Some(3) => {
            s.push_str("start=");
            s.push_str(&hex_text(p.start));
            s.push_str(" want ");
            s.push_str(&hex_text(want.start));
        }
        Some(4) => {
            s.push_str("pages=");
            s.push_str(&u32_text(p.pages as u32));
            s.push_str(" want ");
            s.push_str(&u32_text(want.pages as u32));
        }
        Some(5) => {
            s.push_str("phdr=");
            s.push_str(&hex_text(p.phdr));
            s.push_str(" want ");
            s.push_str(&hex_text(want.phdr));
        }
        _ => {
            s.push_str("segments=");
            s.push_str(&u32_text(p.segs));
            s.push_str(" want ");
            s.push_str(&u32_text(want.segs));
        }
    }
    s.push('\n');
    s
}

/// 单行诊断。
pub fn message(f: Fault, img: &Image, phoff: u64, e_entry: u64) -> String {
    let mut s = String::new();
    match f {
        Fault::Ok => match plan(img, phoff, e_entry) {
            Ok(p) => {
                s.push_str("ok: bias=");
                s.push_str(&hex_text(p.bias));
                s.push_str(" entry=");
                s.push_str(&hex_text(p.entry));
                s.push_str(" brk=");
                s.push_str(&hex_text(p.brk));
                s.push_str(" pages=");
                s.push_str(&u32_text(p.pages as u32));
                s.push('\n');
            }
            Err(_) => {
                s.push_str("ok: plan unavailable\n");
            }
        },
        Fault::Seg => {
            s.push_str("./prog: ELF load plan: no usable loadable segments\n");
        }
        Fault::Base => {
            s.push_str("./prog: ELF load plan: mixed load bias");
            if let Some(b) = img.unified_bias() {
                s.push_str(" ");
                s.push_str(&hex_text(b));
            }
            s.push('\n');
        }
        Fault::Entry => {
            if img.is_empty() {
                s.push_str("./prog: ELF load plan: no entry\n");
            } else {
                let b = img.unified_bias().unwrap_or(0);
                s.push_str("./prog: ELF load plan: entry ");
                s.push_str(&hex_text(e_entry.wrapping_add(b)));
                s.push_str(" not in executable segment\n");
            }
        }
        Fault::Brk => {
            s.push_str("./prog: ELF load plan: brk overflows u64\n");
        }
        Fault::Phdr => match plan(img, phoff, e_entry) {
            Ok(p) => {
                s.push_str("./prog: ELF load plan: phdr at ");
                s.push_str(&hex_text(p.phdr));
                s.push_str(" outside mapped image\n");
            }
            Err(_) => {
                let b = img.unified_bias().unwrap_or(0);
                s.push_str("./prog: ELF load plan: phdr at ");
                s.push_str(&hex_text(phoff.wrapping_add(b)));
                s.push_str(" outside mapped image\n");
            }
        },
        Fault::Drift => {
            s.push_str("./prog: ELF load plan drift\n");
        }
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
    /// 程序头文件偏移。
    pub phoff: u64,
    /// 入口字段（相对装入前）。
    pub entry: u64,
    /// 期望判面。
    pub fault: Fault,
    /// 备注。
    pub note: &'static str,
}

impl Slot {
    /// 构造。
    pub const fn new(img: Image, phoff: u64, entry: u64, fault: Fault, note: &'static str) -> Slot {
        Slot { img, phoff, entry, fault, note }
    }
}

/// 十四槽语料：八活六死。
pub const CASES: [Slot; CORPUS] = [
    Slot::new(
        Image::three(Seg::text(), Seg::rodata(), Seg::data()),
        0x40,
        0x80,
        Fault::Ok,
        "classic-three",
    ),
    Slot::new(Image::one(Seg::text()), 0x40, 0x80, Fault::Ok, "single-text"),
    Slot::new(
        Image::two(
            Seg::new(PT_LOAD, 0, 0, 0x100, 0x100, PF_RX, PAGE),
            Seg::new(PT_LOAD, 0x1000, 0x1000, 0x100, 0x100, PF_RW, PAGE),
        ),
        0x40,
        0x80,
        Fault::Ok,
        "pie-zero-bias",
    ),
    Slot::new(Image::two(Seg::text(), Seg::rodata()), 0x40, 0, Fault::Ok, "text+rodata"),
    Slot::new(
        Image::four(
            Seg::text(),
            Seg::rodata(),
            Seg::data(),
            Seg::new(PT_LOAD, 0x3000, 0x403000, 0x40, 0x400, PF_RW, PAGE),
        ),
        0x40,
        0x80,
        Fault::Ok,
        "four-seg-bss",
    ),
    Slot::new(Image::one(Seg::text()), 0x40, 0, Fault::Ok, "entry-at-start"),
    Slot::new(Image::one(Seg::text()), 0x40, 0xff, Fault::Ok, "entry-last-byte"),
    Slot::new(
        Image::two(
            Seg::text(),
            Seg::new(PT_LOAD, 0x1000, 0x401000, 0, 0x3000, PF_RW, PAGE),
        ),
        0x40,
        0x80,
        Fault::Ok,
        "text+bss",
    ),
    Slot::new(
        Image::three(Seg::text(), Seg::rodata(), Seg::data()),
        0x40,
        0x1040,
        Fault::Entry,
        "entry-nonexec",
    ),
    Slot::new(
        Image::three(Seg::text(), Seg::rodata(), Seg::data()),
        0x40,
        0x100000,
        Fault::Entry,
        "entry-outside",
    ),
    Slot::new(
        Image::two(Seg::text(), Seg::new(PT_LOAD, 0x1000, 0x402000, 0x10, 0x10, PF_RW, PAGE)),
        0x40,
        0x80,
        Fault::Base,
        "bias-split",
    ),
    Slot::new(
        Image::one(Seg::new(PT_LOAD, 0, 0xffff_ffff_ffff_f000, 0x800, 0x800, PF_RX, PAGE)),
        0x40,
        0,
        Fault::Brk,
        "brk-overflow",
    ),
    Slot::new(
        Image::three(Seg::text(), Seg::rodata(), Seg::data()),
        0x8000,
        0x80,
        Fault::Phdr,
        "phdr-outside",
    ),
    Slot::new(Image::new(), 0x40, 0x80, Fault::Seg, "empty-image"),
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
            if a.note == b.note || (a.img == b.img && a.phoff == b.phoff && a.entry == b.entry) {
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
        let got = match plan(&s.img, s.phoff, s.entry) {
            Ok(_) => Fault::Ok,
            Err(f) => f,
        };
        if got != s.fault {
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
    /// 结构。
    pub seg: u32,
    /// 偏置。
    pub base: u32,
    /// 入口。
    pub entry: u32,
    /// brk。
    pub brk: u32,
    /// 程序头。
    pub phdr: u32,
    /// 对判命中。
    pub matched: u32,
}

impl Report {
    /// 空面。
    pub const fn new() -> Report {
        Report { total: 0, accept: 0, reject: 0, seg: 0, base: 0, entry: 0, brk: 0, phdr: 0, matched: 0 }
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
            return Err(LD_ENOBS);
        }
        if self.matched != self.total {
            return Err(LD_EVER);
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
            Fault::Seg => self.seg += 1,
            Fault::Base => self.base += 1,
            Fault::Entry => self.entry += 1,
            Fault::Brk => self.brk += 1,
            Fault::Phdr => self.phdr += 1,
            _ => {}
        }
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("anchor corpus=");
        s.push_str(&u32_text(self.total));
        s.push_str(" accept=");
        s.push_str(&u32_text(self.accept));
        s.push_str(" reject=");
        s.push_str(&u32_text(self.reject));
        s.push_str(" seg=");
        s.push_str(&u32_text(self.seg));
        s.push_str(" base=");
        s.push_str(&u32_text(self.base));
        s.push_str(" entry=");
        s.push_str(&u32_text(self.entry));
        s.push_str(" brk=");
        s.push_str(&u32_text(self.brk));
        s.push_str(" phdr=");
        s.push_str(&u32_text(self.phdr));
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
            return Err(LD_ESLOT);
        }
    }
    let mut r = Report::new();
    let mut i = 0usize;
    while i < CASES.len() {
        let slot = match CASES.get(i) {
            Some(v) => *v,
            None => return Err(LD_ESLOT),
        };
        let got = match plan(&slot.img, slot.phoff, slot.entry) {
            Ok(_) => Fault::Ok,
            Err(f) => f,
        };
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
        Err(LD_ENOBS)
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

    /// 规划一图。
    pub fn check(&mut self, img: &Image, phoff: u64, e_entry: u64) -> Result<Plan, u32> {
        if self.used {
            self.last = LD_ESTATE;
            return Err(LD_ESTATE);
        }
        self.used = true;
        self.steps = 1;
        match plan(img, phoff, e_entry) {
            Ok(p) => {
                self.last = 0;
                Ok(p)
            }
            Err(f) => {
                self.last = f.code();
                Err(f.code())
            }
        }
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
            return Err(LD_ESEAL);
        }
        if self.len >= SAMPLES {
            return Err(LD_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
            self.len += 1;
            Ok(())
        } else {
            Err(LD_ESLOT)
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
            return Err(LD_EBAL);
        }
        const VALS: [u32; 8] = [14, 8, 6, 1, 1, 2, 1, 929];
        const CODES: [u32; 6] = [LD_EENTRY, LD_EENTRY, LD_EBASE, LD_EBRK, LD_EPHDR, LD_ESEG];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(LD_EBAL),
            };
            if s.slot as usize != i {
                return Err(LD_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(LD_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(LD_EVALUE);
                        }
                    }
                    None => return Err(LD_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(LD_EVALUE);
                        }
                    }
                    None => return Err(LD_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(LD_EBAL);
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
            return Err(LD_ESEAL);
        }
        if self.len != SAMPLES {
            return Err(LD_EBAL);
        }
        self.audit()?;
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if !self.frozen {
            return Err(LD_ESTATE);
        }
        let again = self.seal();
        if again != self.root {
            return Err(LD_ESEAL);
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
    let _ = l.register(count_cell(3, OP_PAGE, r.map(|x| x.seg as usize)));

    let _ = l.register(count_cell(4, OP_BIAS, r.map(|x| x.base as usize)));
    let _ = l.register(count_cell(5, OP_ENTRY, r.map(|x| x.entry as usize)));
    let _ = l.register(count_cell(6, OP_BRK, r.map(|x| x.brk as usize)));

    let red = run_with(Some((0, LD_EENTRY)));
    let _ = l.register(count_cell(7, OP_TEXT, red.map(|x| x.rate() as usize)));

    let en1 = CASES[8].fault;
    let _ = l.register(err_cell(8, OP_ENTRY, en1.code()));

    let en2 = CASES[9].fault;
    let _ = l.register(err_cell(9, OP_ENTRY, en2.code()));

    let bs = CASES[10].fault;
    let _ = l.register(err_cell(10, OP_BIAS, bs.code()));

    let bk = CASES[11].fault;
    let _ = l.register(err_cell(11, OP_BRK, bk.code()));

    let ph = CASES[12].fault;
    let _ = l.register(err_cell(12, OP_PHDR, ph.code()));

    let sg = CASES[13].fault;
    let _ = l.register(err_cell(13, OP_PAGE, sg.code()));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 名称与段基址。
fn name_ok() -> bool {
    let names = [
        Fault::Ok.name(),
        Fault::Seg.name(),
        Fault::Base.name(),
        Fault::Entry.name(),
        Fault::Brk.name(),
        Fault::Phdr.name(),
        Fault::Drift.name(),
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
    LD_ESEG == SEG_BASE + 0x01
        && LD_EBASE == SEG_BASE + 0x02
        && LD_EENTRY == SEG_BASE + 0x03
        && LD_EBRK == SEG_BASE + 0x04
        && LD_EPHDR == SEG_BASE + 0x05
        && LD_EDRIFT == SEG_BASE + 0x06
        && LD_ESTATE == SEG_BASE + 0x07
        && LD_EVER == SEG_BASE + 0x08
        && LD_EBAL == SEG_BASE + 0x09
        && LD_ESLOT == SEG_BASE + 0x0a
        && LD_EVALUE == SEG_BASE + 0x0b
        && LD_ESEAL == SEG_BASE + 0x0c
        && LD_ENOBS == SEG_BASE + 0x0d
}

/// 页算术与映射。
fn page_ok() -> bool {
    let a = page_floor(0x4000ff) == 0x400000 && page_ceil(0x400001) == 0x401000;
    let b = page_ceil(0x400000) == 0x400000 && page_ceil(0) == 0;
    let c = bias_of(0x401000, 0x1000) == 0x400000 && bias_of(0x402000, 0) == 0x402000;
    let img = Image::three(Seg::text(), Seg::rodata(), Seg::data());
    let d = mapping_at(&img, 0x400040).is_some() && mapping_at(&img, 0x401040).is_some();
    let e = mapping_at(&img, 0x403000).is_none() && mapping_at(&img, 0x3fffff).is_none();
    let f = exec_at(&img, 0x400080).is_some() && exec_at(&img, 0x401080).is_none();
    let g = page_ceil(u64::MAX) == u64::MAX;
    a && b && c && d && e && f && g
}

/// 正件计划。
fn anchor_ok() -> bool {
    let img = Image::three(Seg::text(), Seg::rodata(), Seg::data());
    let p = match plan(&img, 0x40, 0x80) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = p.matches(&Plan::anchor()) && compare(&p, &Plan::anchor()) == Ok(());
    let b = p.bias == 0x400000 && p.entry == 0x400080 && p.brk == 0x403000;
    let c = p.start == 0x400000 && p.pages == 3 && p.phdr == 0x400040 && p.segs == 3;
    let d = p.drift(&Plan::anchor()).is_none();
    a && b && c && d
}

/// 单段与 PIE。
fn single_ok() -> bool {
    let one = Image::one(Seg::text());
    let p = match plan(&one, 0x40, 0x80) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = p.matches(&Plan::single()) && p.pages == 1;
    let pie = Image::two(
        Seg::new(PT_LOAD, 0, 0, 0x100, 0x100, PF_RX, PAGE),
        Seg::new(PT_LOAD, 0x1000, 0x1000, 0x100, 0x100, PF_RW, PAGE),
    );
    let q = match plan(&pie, 0x40, 0x80) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let b = q.matches(&Plan::pie()) && q.bias == 0 && q.entry == 0x80;
    let c = q.start == 0 && q.pages == 2 && q.phdr == 0x40;
    a && b && c
}

/// 四段计划。
fn four_ok() -> bool {
    let img = Image::four(
        Seg::text(),
        Seg::rodata(),
        Seg::data(),
        Seg::new(PT_LOAD, 0x3000, 0x403000, 0x40, 0x400, PF_RW, PAGE),
    );
    let p = match plan(&img, 0x40, 0x80) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = p.segs == 4 && p.brk == 0x404000 && p.pages == 4;
    let b = p.entry == 0x400080 && p.phdr == 0x400040;
    let c = p.start == 0x400000;
    a && b && c
}

/// 偏置门。
fn base_ok() -> bool {
    let split = Image::two(Seg::text(), Seg::new(PT_LOAD, 0x1000, 0x402000, 0x10, 0x10, PF_RW, PAGE));
    let a = plan(&split, 0x40, 0x80).err() == Some(Fault::Base);
    let b = split.unified_bias().is_none();
    let c = Fault::Base.code() == LD_EBASE && Fault::Base.errno() == ENOEXEC;
    let d = plan(&Image::one(Seg::text()), 0x40, 0x80).is_ok();
    a && b && c && d
}

/// 入口门。
fn entry_ok() -> bool {
    let img = Image::three(Seg::text(), Seg::rodata(), Seg::data());
    let a = plan(&img, 0x40, 0x80).is_ok();
    let ro = Image::two(Seg::text(), Seg::new(PT_LOAD, 0x1000, 0x401000, 0x100, 0x100, PF_RO, PAGE));
    let b = plan(&ro, 0x40, 0x80).is_ok();
    let c = plan(&Image::new(), 0x40, 0x80).err() == Some(Fault::Seg);
    let d = Fault::Entry.code() == LD_EENTRY && Fault::Entry.errno() == EINVAL;
    let e = exec_at(&img, 0x4000ff).is_some() && exec_at(&img, 0x400100).is_none();
    a && b && c && d && e
}

/// brk 门。
fn brk_ok() -> bool {
    let big = Image::one(Seg::new(PT_LOAD, 0, 0xffff_ffff_ffff_f000, 0x800, 0x800, PF_RX, PAGE));
    let a = plan(&big, 0x40, 0).err() == Some(Fault::Brk);
    let b = Fault::Brk.code() == LD_EBRK;
    let img = Image::two(Seg::text(), Seg::new(PT_LOAD, 0x1000, 0x401000, 0, 0x3000, PF_RW, PAGE));
    let p = match plan(&img, 0x40, 0x80) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let c = p.brk == 0x404000 && p.pages == 4;
    let d = page_ceil(0x401100) == 0x402000;
    a && b && c && d
}

/// 程序头门。
fn phdr_ok() -> bool {
    let img = Image::three(Seg::text(), Seg::rodata(), Seg::data());
    let a = plan(&img, 0x40, 0x80).is_ok();
    let b = plan(&img, 0x8000, 0x80).err() == Some(Fault::Phdr);
    let c = plan(&img, 0x1000, 0x80).is_ok();
    let d = Fault::Phdr.code() == LD_EPHDR;
    let e = plan(&img, u64::MAX, 0x80).err() == Some(Fault::Phdr);
    a && b && c && d && e
}

/// 结构门。
fn struct_ok() -> bool {
    let a = plan(&Image::new(), 0x40, 0x80).err() == Some(Fault::Seg);
    let b = plan(&Image::one(Seg::new(PT_DYNAMIC, 0x40, 0x400040, 0x10, 0x10, PF_R, PAGE)), 0x40, 0x80)
        .err()
        == Some(Fault::Seg);
    let c = plan(&Image::one(Seg::new(PT_LOAD, 0, 0x400000, 0, 0, PF_RX, PAGE)), 0x40, 0x80).err()
        == Some(Fault::Seg);
    let d = Fault::Seg.code() == LD_ESEG && Fault::Seg.errno() == ENOEXEC;
    a && b && c && d
}

/// 漂移门。
fn drift_ok() -> bool {
    let img = Image::three(Seg::text(), Seg::rodata(), Seg::data());
    let p = match plan(&img, 0x40, 0x80) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let mut want = Plan::anchor();
    let a = p.drift(&want).is_none() && compare(&p, &want) == Ok(());
    want.entry = 0x400081;
    let b = p.drift(&want) == Some(1) && compare(&p, &want) == Err(LD_EDRIFT);
    let c = drift_message(&p, &want).as_str()
        == "./prog: ELF load plan drift: entry=0x400080 want 0x400081\n";
    let mut w2 = Plan::anchor();
    w2.pages = 9;
    let d = p.drift(&w2) == Some(4) && drift_message(&p, &w2).as_str()
        == "./prog: ELF load plan drift: pages=3 want 9\n";
    a && b && c && d
}

/// 文案。
fn message_ok() -> bool {
    let img = Image::three(Seg::text(), Seg::rodata(), Seg::data());
    let a = message(Fault::Ok, &img, 0x40, 0x80).as_str()
        == "ok: bias=0x400000 entry=0x400080 brk=0x403000 pages=3\n";
    let b = message(Fault::Base, &Image::two(Seg::text(), Seg::new(PT_LOAD, 0x1000, 0x402000, 0x10, 0x10, PF_RW, PAGE)), 0x40, 0x80).as_str()
        == "./prog: ELF load plan: mixed load bias\n";
    let c = message(Fault::Brk, &Image::one(Seg::new(PT_LOAD, 0, 0x400000, 0x10, u64::MAX, PF_RX, PAGE)), 0x40, 0).as_str()
        == "./prog: ELF load plan: brk overflows u64\n";
    let d = message(Fault::Phdr, &img, 0x8000, 0x80).as_str()
        == "./prog: ELF load plan: phdr at 0x408000 outside mapped image\n";
    let e = message(Fault::Seg, &Image::new(), 0x40, 0x80).as_str()
        == "./prog: ELF load plan: no usable loadable segments\n";
    a && b && c && d && e
}

/// 后半文案。
fn tail_message_ok() -> bool {
    let img = Image::three(Seg::text(), Seg::rodata(), Seg::data());
    let a = message(Fault::Entry, &img, 0x40, 0x1040).as_str()
        == "./prog: ELF load plan: entry 0x401040 not in executable segment\n";
    let b = message(Fault::Drift, &img, 0x40, 0x80).as_str()
        == "./prog: ELF load plan drift\n";
    let c = Plan::anchor().seal() == Plan::anchor().seal();
    let d = Plan::anchor().seal() != Plan::single().seal();
    let e = Plan::anchor().matches(&Plan::anchor()) && !Plan::anchor().matches(&Plan::single());
    a && b && c && d && e
}

/// 语料与汇总。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.total == 14 && r.accept == 8 && r.reject == 6 && r.matched == 14;
    let b = r.seg == 1 && r.base == 1 && r.entry == 2 && r.brk == 1 && r.phdr == 1;
    let c = r.rate() == 1000 && r.green() && r.gate() == Ok(());
    let d = table_ok() && unique() && notes_ok();
    a && b && c && d
}

/// 槽号与覆盖。
fn slots_ok() -> bool {
    let a = match slot_at(0) {
        Some(s) => s.fault == Fault::Ok && s.phoff == 0x40,
        None => false,
    };
    let b = match slot_at(13) {
        Some(s) => s.fault == Fault::Seg && s.img.is_empty(),
        None => false,
    };
    let c = slot_at(CORPUS).is_none();
    let d = cover(CORPUS) == Ok(()) && cover(1) == Err(LD_ENOBS);
    let e = short(Fault::Drift) == "drift" && short(Fault::Ok) == "ok";
    a && b && c && d && e
}

/// 红档。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, LD_EENTRY))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched == CORPUS as u32 - 1 && r.rate() == 929;
    let b = r.gate() == Err(LD_EVER) && !r.green();
    let c = run_with(Some((CORPUS, LD_EENTRY))).err() == Some(LD_ESLOT);
    a && b && c
}

/// 校验器。
fn checker_ok() -> bool {
    let mut c = Checker::new();
    let a = !c.bound() && c.steps() == 0;
    let img = Image::three(Seg::text(), Seg::rodata(), Seg::data());
    let b = match c.check(&img, 0x40, 0x80) {
        Ok(p) => p.matches(&Plan::anchor()) && c.last() == 0,
        Err(_) => false,
    };
    let d = c.check(&img, 0x40, 0x80).err() == Some(LD_ESTATE);
    let e = c.bound() && c.steps() == 1 && c.last() == LD_ESTATE;
    a && b && d && e
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && page_ok();
    out[1] = anchor_ok() && single_ok() && four_ok();
    out[2] = base_ok() && entry_ok() && brk_ok() && phdr_ok() && struct_ok() && drift_ok();
    out[3] = message_ok() && tail_message_ok() && corpus_ok() && slots_ok()
        && reds_ok() && checker_ok() && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11615_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11615.load_anchor");

    s.add(
        "AH01-名称与段基址",
        {
            name_ok()
        },
        "static",
    );

    s.add(
        "AH02-页算术与映射",
        {
            page_ok()
        },
        "static",
    );

    s.add(
        "AH03-正件计划锚",
        {
            anchor_ok()
        },
        "static",
    );

    s.add(
        "AH04-单段与 PIE",
        {
            single_ok()
        },
        "static",
    );

    s.add(
        "AH05-四段计划",
        {
            four_ok()
        },
        "static",
    );

    s.add(
        "AH06-偏置门",
        {
            base_ok()
        },
        "static",
    );

    s.add(
        "AH07-入口门",
        {
            entry_ok()
        },
        "static",
    );

    s.add(
        "AH08-brk 门",
        {
            brk_ok()
        },
        "static",
    );

    s.add(
        "AH09-程序头门",
        {
            phdr_ok()
        },
        "static",
    );

    s.add(
        "AH10-结构门",
        {
            struct_ok()
        },
        "static",
    );

    s.add(
        "AH11-文案与漂移",
        {
            message_ok() && tail_message_ok() && drift_ok()
        },
        "static",
    );

    s.add(
        "AH12-语料与汇总",
        {
            corpus_ok() && slots_ok() && reds_ok() && checker_ok()
        },
        "static",
    );

    s.add(
        "AH13-冻结重放四注入",
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
        panic!("unx11615 test fail")
    }

    fn classic() -> Image {
        Image::three(Seg::text(), Seg::rodata(), Seg::data())
    }

    #[test]
    fn paging() {
        assert_eq!(page_floor(0x4000ff), 0x400000);
        assert_eq!(page_ceil(0x400001), 0x401000);
        assert_eq!(page_ceil(0x400000), 0x400000);
        assert_eq!(page_ceil(0), 0);
        assert_eq!(page_ceil(u64::MAX), u64::MAX);
        assert_eq!(bias_of(0x401000, 0x1000), 0x400000);
    }

    #[test]
    fn mapping() {
        let img = classic();
        assert!(mapping_at(&img, 0x400000).is_some());
        assert!(mapping_at(&img, 0x4000ff).is_some());
        assert!(mapping_at(&img, 0x400100).is_none());
        assert!(mapping_at(&img, 0x401020).is_some());
        assert!(mapping_at(&img, 0x3fffff).is_none());
        assert!(exec_at(&img, 0x400040).is_some());
        assert!(exec_at(&img, 0x401040).is_none());
    }

    #[test]
    fn anchor_plan() {
        let p = match plan(&classic(), 0x40, 0x80) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert!(p.matches(&Plan::anchor()));
        assert_eq!(p.bias, 0x400000);
        assert_eq!(p.entry, 0x400080);
        assert_eq!(p.brk, 0x403000);
        assert_eq!(p.start, 0x400000);
        assert_eq!(p.pages, 3);
        assert_eq!(p.phdr, 0x400040);
        assert_eq!(p.segs, 3);
    }

    #[test]
    fn single_plan() {
        let p = match plan(&Image::one(Seg::text()), 0x40, 0x80) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert!(p.matches(&Plan::single()));
        assert_eq!(p.pages, 1);
        assert_eq!(p.brk, 0x401000);
    }

    #[test]
    fn pie_plan() {
        let pie = Image::two(
            Seg::new(PT_LOAD, 0, 0, 0x100, 0x100, PF_RX, PAGE),
            Seg::new(PT_LOAD, 0x1000, 0x1000, 0x100, 0x100, PF_RW, PAGE),
        );
        let p = match plan(&pie, 0x40, 0x80) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert!(p.matches(&Plan::pie()));
        assert_eq!(p.bias, 0);
        assert_eq!(p.entry, 0x80);
    }

    #[test]
    fn four_seg_plan() {
        let img = Image::four(
            Seg::text(),
            Seg::rodata(),
            Seg::data(),
            Seg::new(PT_LOAD, 0x3000, 0x403000, 0x40, 0x400, PF_RW, PAGE),
        );
        let p = match plan(&img, 0x40, 0x80) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(p.segs, 4);
        assert_eq!(p.brk, 0x404000);
        assert_eq!(p.pages, 4);
    }

    #[test]
    fn base_fault() {
        let img = Image::two(Seg::text(), Seg::new(PT_LOAD, 0x1000, 0x402000, 0x10, 0x10, PF_RW, PAGE));
        assert_eq!(plan(&img, 0x40, 0x80).err(), Some(Fault::Base));
        assert_eq!(Fault::Base.code(), LD_EBASE);
    }

    #[test]
    fn entry_fault() {
        let img = Image::two(Seg::text(), Seg::new(PT_LOAD, 0x1000, 0x401000, 0x100, 0x100, PF_RO, PAGE));
        assert_eq!(plan(&img, 0x40, 0x80).is_ok(), true);
        assert_eq!(plan(&img, 0x40, 0x1080).err(), Some(Fault::Entry));
    }

    #[test]
    fn brk_fault() {
        let img = Image::one(Seg::new(PT_LOAD, 0, 0xffff_ffff_ffff_f000, 0x800, 0x800, PF_RX, PAGE));
        assert_eq!(plan(&img, 0x40, 0).err(), Some(Fault::Brk));
        assert_eq!(Fault::Brk.code(), LD_EBRK);
    }

    #[test]
    fn phdr_fault() {
        let a = plan(&classic(), 0x8000, 0x80).err();
        assert_eq!(a, Some(Fault::Phdr));
        assert_eq!(plan(&classic(), u64::MAX, 0x80).err(), Some(Fault::Phdr));
        assert_eq!(Fault::Phdr.code(), LD_EPHDR);
    }

    #[test]
    fn struct_fault() {
        assert_eq!(plan(&Image::new(), 0x40, 0x80).err(), Some(Fault::Seg));
        let dyn_seg = Seg::new(PT_DYNAMIC, 0x40, 0x400040, 0x10, 0x10, PF_R, PAGE);
        assert_eq!(plan(&Image::one(dyn_seg), 0x40, 0x80).err(), Some(Fault::Seg));
        let empty_seg = Seg::new(PT_LOAD, 0, 0x400000, 0, 0, PF_RX, PAGE);
        assert_eq!(plan(&Image::one(empty_seg), 0x40, 0x80).err(), Some(Fault::Seg));
    }

    #[test]
    fn drift_detection() {
        let p = match plan(&classic(), 0x40, 0x80) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        let mut want = Plan::anchor();
        assert_eq!(p.drift(&want), None);
        want.entry = 0x400081;
        assert_eq!(p.drift(&want), Some(1));
        assert_eq!(compare(&p, &want), Err(LD_EDRIFT));
        assert_eq!(
            drift_message(&p, &want).as_str(),
            "./prog: ELF load plan drift: entry=0x400080 want 0x400081\n"
        );
    }

    #[test]
    fn message_ok() {
        assert_eq!(
            message(Fault::Ok, &classic(), 0x40, 0x80).as_str(),
            "ok: bias=0x400000 entry=0x400080 brk=0x403000 pages=3\n"
        );
        assert_eq!(
            message(Fault::Seg, &Image::new(), 0x40, 0x80).as_str(),
            "./prog: ELF load plan: no usable loadable segments\n"
        );
        assert_eq!(
            message(Fault::Phdr, &classic(), 0x8000, 0x80).as_str(),
            "./prog: ELF load plan: phdr at 0x408000 outside mapped image\n"
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
        assert_eq!(r.seg, 1);
        assert_eq!(r.base, 1);
        assert_eq!(r.entry, 2);
        assert_eq!(r.brk, 1);
        assert_eq!(r.phdr, 1);
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
            "anchor corpus=14 accept=8 reject=6 seg=1 base=1 entry=2 brk=1 phdr=1 match=14 rate=1000ppm\n"
        );
    }

    #[test]
    fn red_override() {
        let r = match run_with(Some((0, LD_EENTRY))) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(r.matched, 13);
        assert_eq!(r.rate(), 929);
        assert_eq!(r.gate(), Err(LD_EVER));
        assert_eq!(run_with(Some((14, LD_EENTRY))).err(), Some(LD_ESLOT));
    }

    #[test]
    fn checker_once() {
        let mut c = Checker::new();
        match c.check(&classic(), 0x40, 0x80) {
            Ok(p) => assert!(p.matches(&Plan::anchor())),
            Err(_) => fail(),
        }
        assert_eq!(c.check(&classic(), 0x40, 0x80).err(), Some(LD_ESTATE));
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
        assert_eq!(l.register(Sample::blank()).err(), Some(LD_ESEAL));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.freeze().err(), Some(LD_ESEAL));
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
        assert_eq!(classic().seal(), classic().seal());
        assert_eq!(u32_text(0).as_str(), "0");
        assert_eq!(u32_text(14).as_str(), "14");
        assert_eq!(hex_text(0x400080).as_str(), "0x400080");
        assert_eq!(hex_text(929).as_str(), "0x3a1");
    }

    #[test]
    fn slot_bounds() {
        assert!(slot_at(0).is_some());
        assert!(slot_at(CORPUS - 1).is_some());
        assert!(slot_at(CORPUS).is_none());
        assert_eq!(short(Fault::Ok), "ok");
        assert_eq!(short(Fault::Brk), "brk");
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11615_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }
}
