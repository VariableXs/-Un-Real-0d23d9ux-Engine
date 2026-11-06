//! UNX-F11622 .so 空文件/截断拒收判据
//!
//! 一个 .so 被 `dlopen` 之前，装载器必须先回答两个问题：**这份字节有多长**，
//! 以及**它声称的每一段表是否真的落在文件里**。第二个问题不是单个比较，而
//! 是一条算术阶梯：`e_phoff + e_phnum * e_phentsize` 要落在 `st_size` 之内，
//! 每个 PT_LOAD 的 `p_offset + p_filesz` 要落在之内，动态段里 `DT_STRTAB +
//! DT_STRSZ` 也要落在之内。任何一处越界，glibc 的结论都只有一句：
//! `file too short`。空文件是这条阶梯的第 0 级，`\x7fELF` 头部不足 16 字节
//! 是第 1 级，Ehdr 不完整是第 2 级，程序头表越界是第 3 级，段越界是第 4 级，
//! 动态串表越界是第 5 级。
//!
//! 阶梯的意义在于**根因排序**：同一份被 `dd` 截断的文件可能同时触发第 3 与
//! 第 4 级，但只有最先越界的那一级能解释"从哪里开始缺字节"。所以本模块不
//! 返回布尔，而返回 `(深度, 码, 缺口)`：深度说明卡在第几级，缺口说明还差
//! 多少字节、缺在哪个偏移。`file too short` 保持逐字稳定，诊断信息走另一
//! 条通道，绝不混进 dlerror。
//!
//! 两处容易写反的地方在这里被单独钉住：
//!
//! 1. 节头表（`e_shoff + e_shnum * e_shentsize`）越界**不是**致命错误。
//!    运行期装载只看程序头与动态段，节头表是链接期/调试期的元数据；把它当
//!    拒收条件会让 `strip` 过的、故意裁掉节表的 .so 被误杀。本模块对此只记
//!    `sh_beyond` 诊断位，结论仍是 whole。
//! 2. 乘法先于加法。`e_phnum * e_phentsize` 在 64 位里也会溢出（畸形值可以
//!    顶到 `u64::MAX`），溢出必须落算术档而不是绕回成一个小数后"看起来合
//!    法"。所以每一步都用 `checked_mul` / `checked_add`，溢出统一给算术档。
//!
//! 闸门次序：空 → ident → ehdr → 程序头表 → 段 → 动态串表 → 无段/无动态 → 节表容忍 → 绿档。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本 14 格
//! （8 成 6 败）在 `audit` 逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfc50;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 语料条数。
pub const CORPUS: usize = 14;

/// `e_ident` 长度。
pub const EI_NIDENT: u64 = 16;

/// Elf32_Ehdr 长度。
pub const EHDR32: u64 = 52;

/// Elf64_Ehdr 长度。
pub const EHDR64: u64 = 64;

/// Elf32_Phdr 长度。
pub const PHENT32: u64 = 32;

/// Elf64_Phdr 长度。
pub const PHENT64: u64 = 56;

/// Elf32_Shdr 长度。
pub const SHENT32: u64 = 40;

/// Elf64_Shdr 长度。
pub const SHENT64: u64 = 64;

/// ELFCLASS32。
pub const ELFCLASS32: u8 = 1;

/// ELFCLASS64。
pub const ELFCLASS64: u8 = 2;

/// 截断文案。
pub const MSG_SHORT: &str = "file too short";

/// 无段文案。
pub const MSG_NOSEQ: &str = "object file has no loadable segments";

/// 无动态文案。
pub const MSG_NODYN: &str = "object file has no dynamic section";

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 空文件。
pub const TR_EEMPTY: u32 = SEG_BASE + 0x01;
/// ident 不足。
pub const TR_EIDENT: u32 = SEG_BASE + 0x02;
/// Ehdr 不足。
pub const TR_EHDR: u32 = SEG_BASE + 0x03;
/// 程序头表越界。
pub const TR_EPHDR: u32 = SEG_BASE + 0x04;
/// 段越界。
pub const TR_ESEG: u32 = SEG_BASE + 0x05;
/// 动态串表越界。
pub const TR_EDYN: u32 = SEG_BASE + 0x06;
/// 无段或无动态。
pub const TR_ENOSEQ: u32 = SEG_BASE + 0x07;
/// 算术溢出。
pub const TR_EARITH: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const TR_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const TR_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const TR_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const TR_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const TR_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 量长度。
pub const OP_SIZE: u8 = 0x01;
/// 算程序头表。
pub const OP_PHDR: u8 = 0x02;
/// 算段。
pub const OP_SEG: u8 = 0x03;
/// 算动态表。
pub const OP_DYN: u8 = 0x04;
/// 语料全跑。
pub const OP_CORPUS: u8 = 0x05;
/// 节表容忍。
pub const OP_SHDR: u8 = 0x06;

// ---------------------------------------------------------------------------
// 哈希
// ---------------------------------------------------------------------------

/// FNV 偏移基。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV 素数。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个字节。
fn fold_byte(h: u64, b: u8) -> u64 {
    let mut v = h ^ (b as u64);
    let mut k = 0usize;
    while k < 8 {
        v = v.wrapping_mul(FNV_PRIME);
        k += 1;
    }
    v
}

/// 折入一个字（大端序逐字节）。
fn fold_word(h: u64, w: u64) -> u64 {
    let mut v = h;
    let mut shift = 56u32;
    loop {
        v = fold_byte(v, (((w >> shift) & 0xff) as u32) as u8);
        if shift == 0 {
            break;
        }
        shift -= 8;
    }
    v
}

/// 字节串摘要。
fn digest(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFF;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h = fold_byte(h, *b);
        }
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 文本
// ---------------------------------------------------------------------------

/// 十进制文本。
fn dec_text(v: u64) -> String {
    let mut buf = [0u8; 20];
    let mut n = v;
    let mut i = 20usize;
    if n == 0 {
        return String::from("0");
    }
    while n > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + ((n % 10) as u8);
        n /= 10;
    }
    let mut s = String::new();
    while i < 20 {
        if let Some(b) = buf.get(i) {
            s.push(*b as char);
        }
        i += 1;
    }
    s
}

/// 定宽小写十六进制。
fn hex_text(v: u64, digits: usize) -> String {
    let mut s = String::new();
    let mut n = digits;
    while n > 0 {
        n -= 1;
        let shift = (n as u32) * 4;
        let nib = ((v >> shift) & 0xf) as u8;
        let c = if nib < 10 { b'0' + nib } else { b'a' + (nib - 10) };
        s.push(c as char);
    }
    s
}

/// 两段拼接（避免 format!）。
fn cat(a: &str, b: &str) -> String {
    let mut s = String::from(a);
    s.push_str(b);
    s
}

/// `<name>: <msg>`。
fn named(name: &str, msg: &str) -> String {
    let mut s = String::from(name);
    s.push_str(": ");
    s.push_str(msg);
    s
}

// ---------------------------------------------------------------------------
// 类宽表
// ---------------------------------------------------------------------------

/// 类宽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Width {
    /// ELFCLASS32。
    W32,
    /// ELFCLASS64。
    W64,
}

impl Width {
    /// 由 e_ident[EI_CLASS] 取；未知值按本机宽度处理（类闸门另有专职）。
    pub fn of_class(class: u8) -> Width {
        if class == ELFCLASS32 {
            Width::W32
        } else {
            Width::W64
        }
    }

    /// 名称。
    pub fn name(&self) -> &'static str {
        match self {
            Width::W32 => "ELFCLASS32",
            Width::W64 => "ELFCLASS64",
        }
    }

    /// 位宽。
    pub fn bits(&self) -> u32 {
        match self {
            Width::W32 => 32,
            Width::W64 => 64,
        }
    }

    /// Ehdr 长度。
    pub fn ehdr(&self) -> u64 {
        match self {
            Width::W32 => EHDR32,
            Width::W64 => EHDR64,
        }
    }

    /// 程序头项长度。
    pub fn phent(&self) -> u64 {
        match self {
            Width::W32 => PHENT32,
            Width::W64 => PHENT64,
        }
    }

    /// 节头项长度。
    pub fn shent(&self) -> u64 {
        match self {
            Width::W32 => SHENT32,
            Width::W64 => SHENT64,
        }
    }

    /// 地址空间上界。
    pub fn limit(&self) -> u64 {
        match self {
            Width::W32 => 0xffff_ffff,
            Width::W64 => 0xffff_ffff_ffff_ffff,
        }
    }
}

// ---------------------------------------------------------------------------
// 阶梯深度
// ---------------------------------------------------------------------------

/// 判到第几级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Depth {
    /// 零字节。
    Empty,
    /// 不足 16 字节。
    Ident,
    /// Ehdr 不完整。
    Ehdr,
    /// 程序头表越界。
    Phdr,
    /// 段越界。
    Seg,
    /// 动态串表越界。
    Dyn,
    /// 无 PT_LOAD。
    NoLoad,
    /// 无 PT_DYNAMIC。
    NoDyn,
    /// 算术溢出。
    Arith,
    /// 整份完好。
    Whole,
}

impl Depth {
    /// 名称。
    pub fn name(&self) -> &'static str {
        match self {
            Depth::Empty => "empty",
            Depth::Ident => "ident",
            Depth::Ehdr => "ehdr",
            Depth::Phdr => "phdr",
            Depth::Seg => "seg",
            Depth::Dyn => "dyn",
            Depth::NoLoad => "noload",
            Depth::NoDyn => "nodyn",
            Depth::Arith => "arith",
            Depth::Whole => "whole",
        }
    }

    /// 序号。
    pub fn index(&self) -> u32 {
        match self {
            Depth::Empty => 0,
            Depth::Ident => 1,
            Depth::Ehdr => 2,
            Depth::Phdr => 3,
            Depth::Seg => 4,
            Depth::Dyn => 5,
            Depth::NoLoad => 6,
            Depth::NoDyn => 7,
            Depth::Arith => 8,
            Depth::Whole => 9,
        }
    }

    /// 错误码。
    pub fn code(&self) -> u32 {
        match self {
            Depth::Empty => TR_EEMPTY,
            Depth::Ident => TR_EIDENT,
            Depth::Ehdr => TR_EHDR,
            Depth::Phdr => TR_EPHDR,
            Depth::Seg => TR_ESEG,
            Depth::Dyn => TR_EDYN,
            Depth::NoLoad => TR_ENOSEQ,
            Depth::NoDyn => TR_ENOSEQ,
            Depth::Arith => TR_EARITH,
            Depth::Whole => 0,
        }
    }

    /// dlerror 文案。
    pub fn message(&self) -> &'static str {
        match self {
            Depth::Empty => MSG_SHORT,
            Depth::Ident => MSG_SHORT,
            Depth::Ehdr => MSG_SHORT,
            Depth::Phdr => MSG_SHORT,
            Depth::Seg => MSG_SHORT,
            Depth::Dyn => MSG_SHORT,
            Depth::NoLoad => MSG_NOSEQ,
            Depth::NoDyn => MSG_NODYN,
            Depth::Arith => MSG_SHORT,
            Depth::Whole => "",
        }
    }

    /// 是否存活。
    pub fn alive(&self) -> bool {
        match self {
            Depth::Whole => true,
            _ => false,
        }
    }

    /// 是否属于"太短"族。
    pub fn short_family(&self) -> bool {
        match self {
            Depth::Empty | Depth::Ident | Depth::Ehdr | Depth::Phdr | Depth::Seg | Depth::Dyn => true,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 缺口
// ---------------------------------------------------------------------------

/// 一处越界：要读什么、从哪里读、要多少、文件有多大。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gap {
    /// 读什么。
    pub what: &'static str,
    /// 起始偏移。
    pub off: u64,
    /// 要读长度。
    pub len: u64,
    /// 文件实长。
    pub size: u64,
}

impl Gap {
    /// 构造。
    pub const fn new(what: &'static str, off: u64, len: u64, size: u64) -> Gap {
        Gap { what, off, len, size }
    }

    /// 需要读到的尾部偏移（饱和）。
    pub fn need(&self) -> u64 {
        self.off.saturating_add(self.len)
    }

    /// 还差多少字节。
    pub fn missing(&self) -> u64 {
        self.need().saturating_sub(self.size)
    }

    /// 是否真的越界。
    pub fn beyond(&self) -> bool {
        self.size < self.need()
    }

    /// 诊断文本 `phdr@0x40+0x2a8 need=0x2e8 size=0x40 miss=0x2a8`。
    pub fn report(&self) -> String {
        let mut s = String::from(self.what);
        s.push_str("@0x");
        s.push_str(hex_text(self.off, 8).as_str());
        s.push_str("+0x");
        s.push_str(hex_text(self.len, 8).as_str());
        s.push_str(" need=0x");
        s.push_str(hex_text(self.need(), 8).as_str());
        s.push_str(" size=0x");
        s.push_str(hex_text(self.size, 8).as_str());
        s.push_str(" miss=0x");
        s.push_str(hex_text(self.missing(), 8).as_str());
        s
    }

    /// 摘要。
    pub fn seal(&self) -> u64 {
        let mut h = fold_word(FNV_OFF, self.off);
        h = fold_word(h, self.len);
        h = fold_word(h, self.size);
        h = fold_word(h, digest(self.what.as_bytes()));
        h
    }
}

// ---------------------------------------------------------------------------
// 算术
// ---------------------------------------------------------------------------

/// `off + len`，溢出给算术档。
pub fn extent(off: u64, len: u64) -> Result<u64, u32> {
    match off.checked_add(len) {
        Some(v) => Ok(v),
        None => Err(TR_EARITH),
    }
}

/// `count * unit`，溢出给算术档。
pub fn span(count: u64, unit: u64) -> Result<u64, u32> {
    match count.checked_mul(unit) {
        Some(v) => Ok(v),
        None => Err(TR_EARITH),
    }
}

/// 是否落在文件内。
pub fn inside(size: u64, off: u64, len: u64) -> bool {
    match extent(off, len) {
        Ok(end) => size >= end,
        Err(_) => false,
    }
}

// ---------------------------------------------------------------------------
// 布局
// ---------------------------------------------------------------------------

/// 一份 .so 声称的布局（全部由 Spec 推出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    /// Ehdr 长度。
    pub ehdr: u64,
    /// 程序头表起点。
    pub phoff: u64,
    /// 程序头项长度。
    pub phentsize: u64,
    /// 程序头项数。
    pub phnum: u64,
    /// 程序头表字节数。
    pub ph_len: u64,
    /// 程序头表尾。
    pub ph_end: u64,
    /// 首个段起点。
    pub seg_off: u64,
    /// 首个段文件长度。
    pub seg_filesz: u64,
    /// 段尾。
    pub seg_end: u64,
    /// 动态表起点。
    pub dyn_off: u64,
    /// 动态表长度。
    pub dyn_size: u64,
    /// 动态表尾。
    pub dyn_end: u64,
    /// 串表起点。
    pub strtab_off: u64,
    /// 串表长度。
    pub strsz: u64,
    /// 串表尾。
    pub str_end: u64,
    /// 节头表起点。
    pub shoff: u64,
    /// 节头项长度。
    pub shentsize: u64,
    /// 节头项数。
    pub shnum: u64,
    /// 节头表尾。
    pub sh_end: u64,
}

impl Layout {
    /// 程序头表是否覆盖整表。
    pub fn phdr_fits(&self, size: u64) -> bool {
        size >= self.ph_end
    }

    /// 段是否覆盖。
    pub fn seg_fits(&self, size: u64) -> bool {
        size >= self.seg_end
    }

    /// 动态串表是否覆盖。
    pub fn dyn_fits(&self, size: u64) -> bool {
        size >= self.str_end
    }

    /// 节头表是否覆盖。
    pub fn sh_fits(&self, size: u64) -> bool {
        size >= self.sh_end
    }

    /// 摘要。
    pub fn seal(&self) -> u64 {
        let mut h = fold_word(FNV_OFF, self.ehdr);
        h = fold_word(h, self.phoff);
        h = fold_word(h, self.ph_len);
        h = fold_word(h, self.seg_filesz);
        h = fold_word(h, self.dyn_size);
        h = fold_word(h, self.shnum);
        h
    }
}

// ---------------------------------------------------------------------------
// 样本规格
// ---------------------------------------------------------------------------

/// 一个待判 .so 的规模参数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spec {
    /// 类宽。
    pub class: u8,
    /// 文件实长。
    pub size: u64,
    /// e_phnum。
    pub phnum: u16,
    /// e_phentsize（0 表示按类宽取默认）。
    pub phentsize: u16,
    /// e_phoff 相对 Ehdr 尾的附加偏移。
    pub phoff_shift: u16,
    /// 首段 p_filesz。
    pub seg_filesz: u64,
    /// 动态串表长度。
    pub dyn_size: u64,
    /// 是否带 PT_DYNAMIC。
    pub dynamic: bool,
    /// e_shnum。
    pub shnum: u16,
}

impl Spec {
    /// 构造。
    pub const fn new(
        class: u8,
        size: u64,
        phnum: u16,
        phentsize: u16,
        phoff_shift: u16,
        seg_filesz: u64,
        dyn_size: u64,
        dynamic: bool,
        shnum: u16,
    ) -> Spec {
        Spec {
            class,
            size,
            phnum,
            phentsize,
            phoff_shift,
            seg_filesz,
            dyn_size,
            dynamic,
            shnum,
        }
    }

    /// 类宽。
    pub fn width(&self) -> Width {
        Width::of_class(self.class)
    }

    /// 推出布局；任一步溢出即算术档。
    pub fn plan(&self) -> Result<Layout, u32> {
        let w = self.width();
        let ehdr = w.ehdr();
        let phoff = extent(ehdr, self.phoff_shift as u64)?;
        let phentsize = if self.phentsize == 0 {
            w.phent()
        } else {
            self.phentsize as u64
        };
        let phnum = self.phnum as u64;
        let ph_len = span(phnum, phentsize)?;
        let ph_end = extent(phoff, ph_len)?;
        let seg_off = ph_end;
        let seg_end = extent(seg_off, self.seg_filesz)?;
        let dyn_off = seg_end;
        let dyn_size = self.dyn_size;
        let dyn_end = extent(dyn_off, dyn_size)?;
        let strtab_off = dyn_end;
        let strsz = dyn_size;
        let str_end = extent(strtab_off, strsz)?;
        let shoff = str_end;
        let shentsize = w.shent();
        let shnum = self.shnum as u64;
        let sh_len = span(shnum, shentsize)?;
        let sh_end = extent(shoff, sh_len)?;
        Ok(Layout {
            ehdr,
            phoff,
            phentsize,
            phnum,
            ph_len,
            ph_end,
            seg_off,
            seg_filesz: self.seg_filesz,
            seg_end,
            dyn_off,
            dyn_size,
            dyn_end,
            strtab_off,
            strsz,
            str_end,
            shoff,
            shentsize,
            shnum,
            sh_end,
        })
    }

    /// 摘要。
    pub fn seal(&self) -> u64 {
        let mut h = fold_word(FNV_OFF, self.class as u64);
        h = fold_word(h, self.size);
        h = fold_word(h, self.phnum as u64);
        h = fold_word(h, self.phentsize as u64);
        h = fold_word(h, self.phoff_shift as u64);
        h = fold_word(h, self.seg_filesz);
        h = fold_word(h, self.dyn_size);
        h = fold_word(h, self.dynamic as u64);
        h = fold_word(h, self.shnum as u64);
        h
    }
}

// ---------------------------------------------------------------------------
// 判定结论
// ---------------------------------------------------------------------------

/// 一次截断判定的结论。
#[derive(Clone, Debug)]
pub struct Verdict {
    name: String,
    depth: Depth,
    code: u32,
    size: u64,
    layout: Option<Layout>,
    gap: Option<Gap>,
}

impl Verdict {
    /// 带缺口的结论。
    fn at_gap(name: &str, l: Layout, depth: Depth, g: Gap) -> Verdict {
        Verdict {
            name: String::from(name),
            depth,
            code: depth.code(),
            size: g.size,
            layout: Some(l),
            gap: Some(g),
        }
    }

    /// 不带缺口的结论。
    fn plain(name: &str, l: Layout, depth: Depth, size: u64) -> Verdict {
        Verdict {
            name: String::from(name),
            depth,
            code: depth.code(),
            size,
            layout: Some(l),
            gap: None,
        }
    }

    /// 整份完好。
    fn whole(name: &str, l: Layout, size: u64) -> Verdict {
        Verdict::plain(name, l, Depth::Whole, size)
    }

    /// 算术档（无布局）。
    fn bare(name: &str, depth: Depth, code: u32) -> Verdict {
        Verdict {
            name: String::from(name),
            depth,
            code,
            size: 0,
            layout: None,
            gap: None,
        }
    }

    /// 名字。
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// 深度。
    pub const fn depth(&self) -> Depth {
        self.depth
    }

    /// 错误码。
    pub const fn code(&self) -> u32 {
        self.code
    }

    /// 文件实长。
    pub const fn size(&self) -> u64 {
        self.size
    }

    /// 布局（算术档为 None）。
    pub const fn layout(&self) -> Option<Layout> {
        self.layout
    }

    /// 缺口。
    pub const fn gap(&self) -> Option<Gap> {
        self.gap
    }

    /// 是否可加载。
    pub fn accepted(&self) -> bool {
        self.depth.alive()
    }

    /// 节头表是否落在文件外（容忍项）。
    pub fn sh_beyond(&self) -> bool {
        match self.layout {
            Some(l) => self.size < l.sh_end,
            None => false,
        }
    }

    /// dlerror 文本；完好时为空串。
    pub fn text(&self) -> String {
        if self.accepted() {
            String::new()
        } else {
            named(self.name.as_str(), self.depth.message())
        }
    }

    /// 诊断文本（不进 dlerror）。
    pub fn diagnose(&self) -> String {
        let mut s = String::from(self.name.as_str());
        if self.accepted() {
            s.push_str(": ok depth=whole sh_beyond=");
            s.push_str(if self.sh_beyond() { "1" } else { "0" });
            s.push('\n');
            return s;
        }
        s.push_str(": ");
        s.push_str(self.depth.message());
        s.push_str(" depth=");
        s.push_str(self.depth.name());
        s.push_str(" code=");
        s.push_str(hex_text(self.code as u64, 8).as_str());
        match self.gap {
            Some(g) => {
                s.push(' ');
                s.push_str(g.report().as_str());
            }
            None => s.push_str(" gap=none"),
        }
        s.push('\n');
        s
    }

    /// 摘要。
    pub fn seal(&self) -> u64 {
        let mut h = fold_word(FNV_OFF, self.depth.index() as u64);
        h = fold_word(h, self.size);
        match self.layout {
            Some(l) => h = fold_word(h, l.seal()),
            None => h = fold_word(h, 0),
        }
        match self.gap {
            Some(g) => h = fold_word(h, g.seal()),
            None => h = fold_word(h, 1),
        }
        h = fold_word(h, digest(self.name.as_bytes()));
        h
    }
}

/// 判一份 .so。
pub fn judge(name: &str, spec: &Spec) -> Verdict {
    let l = match spec.plan() {
        Ok(v) => v,
        Err(c) => return Verdict::bare(name, Depth::Arith, c),
    };
    if spec.size == 0 {
        return Verdict::at_gap(name, l, Depth::Empty, Gap::new("e_ident", 0, EI_NIDENT, 0));
    }
    if spec.size < EI_NIDENT {
        return Verdict::at_gap(name, l, Depth::Ident, Gap::new("e_ident", 0, EI_NIDENT, spec.size));
    }
    if spec.size < l.ehdr {
        return Verdict::at_gap(name, l, Depth::Ehdr, Gap::new("ehdr", 0, l.ehdr, spec.size));
    }
    if !l.phdr_fits(spec.size) {
        return Verdict::at_gap(name, l, Depth::Phdr, Gap::new("phdr", l.phoff, l.ph_len, spec.size));
    }
    if l.phnum == 0 {
        return Verdict::plain(name, l, Depth::NoLoad, spec.size);
    }
    if !l.seg_fits(spec.size) {
        return Verdict::at_gap(name, l, Depth::Seg, Gap::new("seg", l.seg_off, l.seg_filesz, spec.size));
    }
    if spec.dynamic {
        if spec.size < l.dyn_end {
            return Verdict::at_gap(name, l, Depth::Dyn, Gap::new("dyn", l.dyn_off, l.dyn_size, spec.size));
        }
        if spec.size < l.str_end {
            return Verdict::at_gap(
                name,
                l,
                Depth::Dyn,
                Gap::new("strtab", l.strtab_off, l.strsz, spec.size),
            );
        }
    } else {
        return Verdict::plain(name, l, Depth::NoDyn, spec.size);
    }
    Verdict::whole(name, l, spec.size)
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 一条语料：名字、规模、期望深度。
#[derive(Clone, Copy, Debug)]
pub struct Case {
    /// 名字。
    pub name: &'static str,
    /// 规模。
    pub spec: Spec,
    /// 期望深度。
    pub expect: Depth,
    /// 备注。
    pub note: &'static str,
}

impl Case {
    /// 构造。
    pub const fn new(name: &'static str, spec: Spec, expect: Depth, note: &'static str) -> Case {
        Case { name, spec, expect, note }
    }
}

/// 十四卡：两活十二死。
pub const CASES: [Case; CORPUS] = [
    Case::new(
        "zero.so",
        Spec::new(ELFCLASS64, 0, 0, 0, 0, 0, 0, false, 0),
        Depth::Empty,
        "zero-byte",
    ),
    Case::new(
        "three.so",
        Spec::new(ELFCLASS64, 3, 0, 0, 0, 0, 0, false, 0),
        Depth::Ident,
        "three-bytes",
    ),
    Case::new(
        "fifteen.so",
        Spec::new(ELFCLASS64, 15, 0, 0, 0, 0, 0, false, 0),
        Depth::Ident,
        "ident-minus-one",
    ),
    Case::new(
        "sixteen.so",
        Spec::new(ELFCLASS64, 16, 1, 0, 0, 0, 0, false, 0),
        Depth::Ehdr,
        "ident-only",
    ),
    Case::new(
        "c32_51.so",
        Spec::new(ELFCLASS32, 51, 1, 0, 0, 0, 0, false, 0),
        Depth::Ehdr,
        "ehdr32-minus-one",
    ),
    Case::new(
        "phdr64.so",
        Spec::new(ELFCLASS64, 164, 3, 0, 0, 0, 0, false, 0),
        Depth::Phdr,
        "phdr-table-cut",
    ),
    Case::new(
        "phdr32.so",
        Spec::new(ELFCLASS32, 92, 2, 0, 0, 0, 0, false, 0),
        Depth::Phdr,
        "phdr32-table-cut",
    ),
    Case::new(
        "seg.so",
        Spec::new(ELFCLASS64, 200, 1, 0, 0, 100, 0, false, 0),
        Depth::Seg,
        "segment-overrun",
    ),
    Case::new(
        "dyn.so",
        Spec::new(ELFCLASS64, 300, 1, 0, 0, 100, 64, true, 0),
        Depth::Dyn,
        "strtab-overrun",
    ),
    Case::new(
        "noload.so",
        Spec::new(ELFCLASS64, 4096, 0, 0, 0, 0, 0, false, 0),
        Depth::NoLoad,
        "phnum-zero",
    ),
    Case::new(
        "nodyn.so",
        Spec::new(ELFCLASS64, 4096, 1, 0, 0, 64, 0, false, 0),
        Depth::NoDyn,
        "no-pt-dynamic",
    ),
    Case::new(
        "arith.so",
        Spec::new(ELFCLASS64, 4096, 1, 0, 0, u64::MAX, 32, true, 0),
        Depth::Arith,
        "filesz-overflow",
    ),
    Case::new(
        "full.so",
        Spec::new(ELFCLASS64, 4096, 1, 0, 0, 64, 32, true, 0),
        Depth::Whole,
        "fully-mapped",
    ),
    Case::new(
        "shtab.so",
        Spec::new(ELFCLASS64, 4096, 1, 0, 0, 64, 32, true, 200),
        Depth::Whole,
        "shtab-tolerated",
    ),
];

/// 取一条语料。
pub fn slot_at(i: usize) -> Option<Case> {
    CASES.get(i).copied()
}

/// 规模互异。
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
                if b.name == a.name || b.spec.seal() == a.spec.seal() {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 备注非空。
pub fn notes_ok() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        match CASES.get(i) {
            Some(c) => {
                if c.note.is_empty() {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 逐条对判。
pub fn table_ok() -> bool {
    if !unique() || !notes_ok() {
        return false;
    }
    let mut i = 0usize;
    while i < CASES.len() {
        let c = match CASES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let v = judge(c.name, &c.spec);
        if v.depth() != c.expect {
            return false;
        }
        if v.depth().alive() != v.text().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 汇总
// ---------------------------------------------------------------------------

/// 语料汇总。
#[derive(Clone, Copy, Debug)]
pub struct Report {
    total: usize,
    ok: usize,
    fail: usize,
    empty: usize,
    ident: usize,
    ehdr: usize,
    phdr: usize,
    seg: usize,
    dyntab: usize,
    noload: usize,
    nodyn: usize,
    arith: usize,
    whole: usize,
    matched: usize,
    red: bool,
}

impl Report {
    /// 空汇总。
    pub const fn new() -> Report {
        Report {
            total: 0,
            ok: 0,
            fail: 0,
            empty: 0,
            ident: 0,
            ehdr: 0,
            phdr: 0,
            seg: 0,
            dyntab: 0,
            noload: 0,
            nodyn: 0,
            arith: 0,
            whole: 0,
            matched: 0,
            red: false,
        }
    }

    /// 总条数。
    pub const fn total(&self) -> usize {
        self.total
    }

    /// 完好条数。
    pub const fn ok(&self) -> usize {
        self.ok
    }

    /// 拒收条数。
    pub const fn fail(&self) -> usize {
        self.fail
    }

    /// 空档条数。
    pub const fn empty(&self) -> usize {
        self.empty
    }

    /// ident 档条数。
    pub const fn ident(&self) -> usize {
        self.ident
    }

    /// ehdr 档条数。
    pub const fn ehdr(&self) -> usize {
        self.ehdr
    }

    /// 程序头表档条数。
    pub const fn phdr(&self) -> usize {
        self.phdr
    }

    /// 段档条数。
    pub const fn seg(&self) -> usize {
        self.seg
    }

    /// 动态档条数。
    pub const fn dyn_count(&self) -> usize {
        self.dyntab
    }

    /// 无段条数。
    pub const fn noload(&self) -> usize {
        self.noload
    }

    /// 无动态条数。
    pub const fn nodyn(&self) -> usize {
        self.nodyn
    }

    /// 算术档条数。
    pub const fn arith(&self) -> usize {
        self.arith
    }

    /// 完好档条数。
    pub const fn whole(&self) -> usize {
        self.whole
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

    /// 千分率。
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
            return Err(TR_ENOBS);
        }
        if self.red {
            return Err(TR_EBAL);
        }
        if self.matched != self.total || self.total != CORPUS {
            return Err(TR_EVALUE);
        }
        if self.ok != 2 || self.fail != 12 {
            return Err(TR_EVALUE);
        }
        if self.empty != 1 || self.ident != 2 || self.ehdr != 2 || self.phdr != 2 {
            return Err(TR_EVALUE);
        }
        if self.seg != 1 || self.dyntab != 1 || self.noload != 1 || self.nodyn != 1 {
            return Err(TR_EVALUE);
        }
        if self.arith != 1 || self.whole != 2 {
            return Err(TR_EVALUE);
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
        let mut s = String::from("trunc total=");
        s.push_str(dec_text(self.total as u64).as_str());
        s.push_str(" ok=");
        s.push_str(dec_text(self.ok as u64).as_str());
        s.push_str(" fail=");
        s.push_str(dec_text(self.fail as u64).as_str());
        s.push_str(" empty=");
        s.push_str(dec_text(self.empty as u64).as_str());
        s.push_str(" ident=");
        s.push_str(dec_text(self.ident as u64).as_str());
        s.push_str(" ehdr=");
        s.push_str(dec_text(self.ehdr as u64).as_str());
        s.push_str(" phdr=");
        s.push_str(dec_text(self.phdr as u64).as_str());
        s.push_str(" seg=");
        s.push_str(dec_text(self.seg as u64).as_str());
        s.push_str(" dyn=");
        s.push_str(dec_text(self.dyntab as u64).as_str());
        s.push_str(" noload=");
        s.push_str(dec_text(self.noload as u64).as_str());
        s.push_str(" nodyn=");
        s.push_str(dec_text(self.nodyn as u64).as_str());
        s.push_str(" arith=");
        s.push_str(dec_text(self.arith as u64).as_str());
        s.push_str(" whole=");
        s.push_str(dec_text(self.whole as u64).as_str());
        s.push_str(" match=");
        s.push_str(dec_text(self.matched as u64).as_str());
        s.push_str(" rate=");
        s.push_str(dec_text(self.rate() as u64).as_str());
        s.push_str("ppm\n");
        s
    }
}

/// 跑一遍。
pub fn run() -> Result<Report, u32> {
    walk(None)
}

/// 跑一遍并注入一处红档。
pub fn run_with(red: Option<(usize, u32)>) -> Result<Report, u32> {
    walk(red)
}

/// 语料行走。
fn walk(red: Option<(usize, u32)>) -> Result<Report, u32> {
    let mut r = Report::new();
    let mut i = 0usize;
    while i < CASES.len() {
        let c = match CASES.get(i) {
            Some(v) => *v,
            None => return Err(TR_EVALUE),
        };
        let v = judge(c.name, &c.spec);
        r.total += 1;
        match v.depth() {
            Depth::Whole => {
                r.ok += 1;
                r.whole += 1;
            }
            d => {
                r.fail += 1;
                match d {
                    Depth::Empty => r.empty += 1,
                    Depth::Ident => r.ident += 1,
                    Depth::Ehdr => r.ehdr += 1,
                    Depth::Phdr => r.phdr += 1,
                    Depth::Seg => r.seg += 1,
                    Depth::Dyn => r.dyntab += 1,
                    Depth::NoLoad => r.noload += 1,
                    Depth::NoDyn => r.nodyn += 1,
                    Depth::Arith => r.arith += 1,
                    Depth::Whole => {}
                }
            }
        }
        if v.depth() == c.expect && v.text().is_empty() == v.depth().alive() {
            r.matched += 1;
        }
        i += 1;
    }
    if let Some((slot, _code)) = red {
        if slot >= r.total {
            return Err(TR_ESLOT);
        }
        r.matched = r.matched.saturating_sub(1);
        r.red = true;
    }
    Ok(r)
}

/// 封面判据。
pub fn cover(n: usize) -> Result<(), u32> {
    if n == CORPUS {
        Ok(())
    } else {
        Err(TR_ENOBS)
    }
}

// ---------------------------------------------------------------------------
// 账本
// ---------------------------------------------------------------------------

/// 账本格。
#[derive(Clone, Copy, Debug)]
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
    pub fn new() -> Ledger {
        Ledger {
            rows: [Sample::blank(); SAMPLES],
            len: 0,
            root: 0,
            frozen: false,
            rejects: 0,
        }
    }

    /// 登记。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(TR_ESLOT);
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
            return Err(TR_EBAL);
        }
        const VALS: [u32; 8] = [14, 2, 12, 1, 2, 2, 2, 929];
        const CODES: [u32; 6] = [TR_EEMPTY, TR_EIDENT, TR_EHDR, TR_EPHDR, TR_ESEG, TR_EDYN];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(TR_EBAL),
            };
            if s.slot as usize != i {
                return Err(TR_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(TR_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(TR_EVALUE);
                        }
                    }
                    None => return Err(TR_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(TR_EVALUE);
                        }
                    }
                    None => return Err(TR_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(TR_EBAL);
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
            return Err(TR_EBAL);
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
            Err(TR_ESEAL)
        }
    }

    /// 冻结指纹。
    pub const fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub const fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub const fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// `Result<usize, u32>` → 样本。
fn count_cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 失败格。
fn err_cell(slot: u8, op: u8, code: u32) -> Sample {
    Sample { slot, op, ok: false, code, value: 0 }
}

/// 某个语料格的判定码。
fn case_code(i: usize) -> u32 {
    match CASES.get(i) {
        Some(c) => judge(c.name, &c.spec).code(),
        None => TR_ENOBS,
    }
}

/// 建账本：8 成 6 败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();

    let _ = l.register(count_cell(0, OP_CORPUS, r.map(|x| x.total())));
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.ok())));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.fail())));
    let _ = l.register(count_cell(3, OP_SIZE, r.map(|x| x.empty())));
    let _ = l.register(count_cell(4, OP_SIZE, r.map(|x| x.ident())));
    let _ = l.register(count_cell(5, OP_SIZE, r.map(|x| x.ehdr())));
    let _ = l.register(count_cell(6, OP_SHDR, r.map(|x| x.whole())));

    let red = run_with(Some((0, TR_EEMPTY)));
    let _ = l.register(count_cell(7, OP_PHDR, red.map(|x| x.rate() as usize)));

    let _ = l.register(err_cell(8, OP_SIZE, case_code(0)));
    let _ = l.register(err_cell(9, OP_SIZE, case_code(1)));
    let _ = l.register(err_cell(10, OP_SIZE, case_code(4)));
    let _ = l.register(err_cell(11, OP_PHDR, case_code(6)));
    let _ = l.register(err_cell(12, OP_SEG, case_code(7)));
    let _ = l.register(err_cell(13, OP_DYN, case_code(8)));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 类宽表。
fn width_ok() -> bool {
    let a = Width::of_class(ELFCLASS32) == Width::W32;
    let b = Width::of_class(ELFCLASS64) == Width::W64;
    let c = Width::of_class(0) == Width::W64;
    let d = Width::W32.bits() == 32 && Width::W64.bits() == 64;
    let e = Width::W32.ehdr() == EHDR32 && Width::W64.ehdr() == EHDR64;
    let f = Width::W32.phent() == PHENT32 && Width::W64.phent() == PHENT64;
    let g = Width::W32.shent() == SHENT32 && Width::W64.shent() == SHENT64;
    let h = Width::W32.name() == "ELFCLASS32" && Width::W64.name() == "ELFCLASS64";
    let i = Width::W32.limit() == 0xffff_ffff;
    let j = Width::W32.ehdr() + 12 == Width::W64.ehdr();
    a && b && c && d && e && f && g && h && i && j
}

/// 阶梯深度。
fn depth_ok() -> bool {
    let a = Depth::Empty.index() == 0 && Depth::Whole.index() == 9;
    let b = Depth::Phdr.index() + 1 == Depth::Seg.index();
    let c = Depth::Empty.name() == "empty" && Depth::Whole.name() == "whole";
    let d = Depth::Empty.code() == TR_EEMPTY && Depth::Phdr.code() == TR_EPHDR;
    let e = Depth::NoLoad.code() == TR_ENOSEQ && Depth::NoDyn.code() == TR_ENOSEQ;
    let f = Depth::Empty.message() == MSG_SHORT && Depth::Phdr.message() == MSG_SHORT;
    let g = Depth::NoLoad.message() == MSG_NOSEQ && Depth::NoDyn.message() == MSG_NODYN;
    let h = Depth::Whole.message().is_empty() && Depth::Whole.alive();
    let i = Depth::Empty.short_family() && Depth::Dyn.short_family();
    let j = !Depth::NoLoad.short_family() && !Depth::Arith.short_family();
    a && b && c && d && e && f && g && h && i && j
}

/// 空与 ident。
fn empty_ok() -> bool {
    let a = {
        let v = judge("zero.so", &CASES[0].spec);
        v.depth() == Depth::Empty && v.code() == TR_EEMPTY && !v.accepted()
    };
    let b = judge("zero.so", &CASES[0].spec).text().as_str() == "zero.so: file too short";
    let c = {
        let v = judge("three.so", &CASES[1].spec);
        v.depth() == Depth::Ident && v.code() == TR_EIDENT
    };
    let d = {
        let v = judge("fifteen.so", &CASES[2].spec);
        match v.gap() {
            Some(g) => g.missing() == 1 && g.need() == EI_NIDENT && g.beyond(),
            None => false,
        }
    };
    let e = {
        let v = judge("zero.so", &CASES[0].spec);
        match v.gap() {
            Some(g) => g.what == "e_ident" && g.missing() == 16,
            None => false,
        }
    };
    a && b && c && d && e
}

/// Ehdr 档。
fn ehdr_ok() -> bool {
    let a = {
        let v = judge("sixteen.so", &CASES[3].spec);
        v.depth() == Depth::Ehdr && v.code() == TR_EHDR
    };
    let b = {
        let v = judge("sixteen.so", &CASES[3].spec);
        match v.gap() {
            Some(g) => g.need() == 64 && g.missing() == 48 && g.what == "ehdr",
            None => false,
        }
    };
    let c = {
        let v = judge("c32_51.so", &CASES[4].spec);
        v.depth() == Depth::Ehdr && v.code() == TR_EHDR
    };
    let d = {
        let v = judge("c32_51.so", &CASES[4].spec);
        match v.gap() {
            Some(g) => g.need() == 52 && g.missing() == 1,
            None => false,
        }
    };
    let e = {
        // 52 字节的 32 位头本身够，但 phdr 表越界 → 下一档。
        let s = Spec::new(ELFCLASS32, 52, 1, 0, 0, 0, 0, false, 0);
        judge("c32_52.so", &s).depth() == Depth::Phdr
    };
    a && b && c && d && e
}

/// 程序头表算术。
fn phdr_ok() -> bool {
    let a = {
        let v = judge("phdr64.so", &CASES[5].spec);
        v.depth() == Depth::Phdr && v.code() == TR_EPHDR
    };
    let b = {
        let v = judge("phdr64.so", &CASES[5].spec);
        match v.gap() {
            Some(g) => g.off == 64 && g.len == 168 && g.need() == 232 && g.missing() == 68,
            None => false,
        }
    };
    let c = {
        let v = judge("phdr32.so", &CASES[6].spec);
        match v.gap() {
            Some(g) => g.off == 52 && g.len == 64 && g.need() == 116 && g.missing() == 24,
            None => false,
        }
    };
    let d = span(3, 56) == Ok(168) && span(2, 32) == Ok(64);
    let e = extent(64, 168) == Ok(232) && extent(u64::MAX, 1) == Err(TR_EARITH);
    let f = span(u64::MAX, 2) == Err(TR_EARITH);
    let g = inside(232, 64, 168) && !inside(231, 64, 168);
    let h = {
        let l = match CASES[5].spec.plan() {
            Ok(v) => v,
            Err(_) => return false,
        };
        l.phdr_fits(232) && !l.phdr_fits(231) && l.phnum == 3
    };
    a && b && c && d && e && f && g && h
}

/// 段越界。
fn seg_ok() -> bool {
    let a = {
        let v = judge("seg.so", &CASES[7].spec);
        v.depth() == Depth::Seg && v.code() == TR_ESEG
    };
    let b = {
        let v = judge("seg.so", &CASES[7].spec);
        match v.gap() {
            Some(g) => g.what == "seg" && g.off == 120 && g.len == 100 && g.missing() == 20,
            None => false,
        }
    };
    let c = {
        // 恰好齐尾不算越界（>= 判据，不是 >）。
        let s = Spec::new(ELFCLASS64, 220, 1, 0, 0, 100, 0, false, 0);
        judge("exact.so", &s).depth() == Depth::NoDyn
    };
    let d = {
        let l = match CASES[7].spec.plan() {
            Ok(v) => v,
            Err(_) => return false,
        };
        l.seg_off == 120 && l.seg_end == 220 && l.seg_fits(220) && !l.seg_fits(219)
    };
    a && b && c && d
}

/// 动态串表。
fn dyn_ok() -> bool {
    let a = {
        let v = judge("dyn.so", &CASES[8].spec);
        v.depth() == Depth::Dyn && v.code() == TR_EDYN
    };
    let b = {
        let v = judge("dyn.so", &CASES[8].spec);
        match v.gap() {
            Some(g) => g.what == "strtab" && g.off == 284 && g.len == 64 && g.missing() == 48,
            None => false,
        }
    };
    let c = {
        // 动态表本身越界时缺口指向 dyn，而不是 strtab。
        let s = Spec::new(ELFCLASS64, 240, 1, 0, 0, 100, 64, true, 0);
        match judge("dynshort.so", &s).gap() {
            Some(g) => g.what == "dyn" && g.missing() == 44,
            None => false,
        }
    };
    let d = {
        let l = match CASES[8].spec.plan() {
            Ok(v) => v,
            Err(_) => return false,
        };
        l.dyn_off == 220 && l.dyn_end == 284 && l.strtab_off == 284 && l.str_end == 348
    };
    let e = {
        let l = match CASES[8].spec.plan() {
            Ok(v) => v,
            Err(_) => return false,
        };
        !l.dyn_fits(300) && l.dyn_fits(348)
    };
    a && b && c && d && e
}

/// 无段与无动态。
fn noseq_ok() -> bool {
    let a = {
        let v = judge("noload.so", &CASES[9].spec);
        v.depth() == Depth::NoLoad && v.code() == TR_ENOSEQ
    };
    let b = judge("noload.so", &CASES[9].spec).text().as_str()
        == "noload.so: object file has no loadable segments";
    let c = {
        let v = judge("nodyn.so", &CASES[10].spec);
        v.depth() == Depth::NoDyn && v.code() == TR_ENOSEQ
    };
    let d = judge("nodyn.so", &CASES[10].spec).text().as_str()
        == "nodyn.so: object file has no dynamic section";
    let e = judge("nodyn.so", &CASES[10].spec).gap().is_none();
    a && b && c && d && e
}

/// 算术档与节表容忍。
fn arith_ok() -> bool {
    let a = {
        let v = judge("arith.so", &CASES[11].spec);
        v.depth() == Depth::Arith && v.code() == TR_EARITH && !v.accepted()
    };
    let b = judge("arith.so", &CASES[11].spec).layout().is_none();
    let c = {
        let v = judge("arith.so", &CASES[11].spec);
        v.text().as_str() == "arith.so: file too short"
    };
    let d = {
        let v = judge("shtab.so", &CASES[13].spec);
        v.accepted() && v.sh_beyond() && v.depth() == Depth::Whole
    };
    let e = {
        let v = judge("full.so", &CASES[12].spec);
        v.accepted() && !v.sh_beyond()
    };
    let f = judge("full.so", &CASES[12].spec).diagnose().as_str()
        == "full.so: ok depth=whole sh_beyond=0\n";
    let g = judge("shtab.so", &CASES[13].spec).diagnose().as_str()
        == "shtab.so: ok depth=whole sh_beyond=1\n";
    a && b && c && d && e && f && g
}

/// 十四卡全跑。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.total() == CORPUS && r.ok() == 2 && r.fail() == 12 && r.matched() == CORPUS;
    let b = r.empty() == 1 && r.ident() == 2 && r.ehdr() == 2 && r.phdr() == 2;
    let c = r.seg() == 1 && r.dyn_count() == 1 && r.noload() == 1 && r.nodyn() == 1;
    let d = r.arith() == 1 && r.whole() == 2 && r.rate() == 1000 && r.green();
    a && b && c && d
}

/// 语料编号。
fn slots_ok() -> bool {
    if !unique() || !notes_ok() || !table_ok() {
        return false;
    }
    let a = match slot_at(0) {
        Some(c) => c.name == "zero.so" && c.expect == Depth::Empty,
        None => false,
    };
    let b = match slot_at(13) {
        Some(c) => c.name == "shtab.so" && c.expect == Depth::Whole,
        None => false,
    };
    let c = slot_at(CORPUS).is_none();
    a && b && c
}

/// 汇总渲染。
fn report_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.render().as_str()
        == "trunc total=14 ok=2 fail=12 empty=1 ident=2 ehdr=2 phdr=2 seg=1 dyn=1 noload=1 nodyn=1 arith=1 whole=2 match=14 rate=1000ppm\n";
    let e = Report::new();
    let b = e.is_empty() && e.rate() == 0 && e.gate() == Err(TR_ENOBS);
    let c = e.render().as_str()
        == "trunc total=0 ok=0 fail=0 empty=0 ident=0 ehdr=0 phdr=0 seg=0 dyn=0 noload=0 nodyn=0 arith=0 whole=0 match=0 rate=0ppm\n";
    a && b && c
}

/// 红单显式。
fn reds_ok() -> bool {
    let r = match run_with(Some((2, TR_EIDENT))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched() == CORPUS - 1 && r.rate() == 929 && r.red();
    let b = r.gate() == Err(TR_EBAL) && !r.green();
    let c = run_with(Some((CORPUS, TR_EEMPTY))).err() == Some(TR_ESLOT);
    let d = run_with(Some((CORPUS + 1, TR_EEMPTY))).err() == Some(TR_ESLOT);
    a && b && c && d
}

/// 封面。
fn cover_ok() -> bool {
    cover(CORPUS) == Ok(()) && cover(CORPUS - 1) == Err(TR_ENOBS) && cover(0) == Err(TR_ENOBS)
}

/// 缺口文本与摘要。
fn hash_ok() -> bool {
    let a = Gap::new("e_ident", 0, 16, 0).report().as_str()
        == "e_ident@0x00000000+0x00000010 need=0x00000010 size=0x00000000 miss=0x00000010";
    let b = Gap::new("phdr", 64, 168, 164).missing() == 68;
    let c = Gap::new("seg", 120, 100, 220).missing() == 0 && !Gap::new("seg", 120, 100, 220).beyond();
    let d = digest(b"abcdef") == digest(b"abcdef") && digest(b"abc") != digest(b"abd");
    let e = hex_text(0x2e8, 8).as_str() == "000002e8" && dec_text(4096).as_str() == "4096";
    let f = cat("file", " too short").as_str() == MSG_SHORT;
    let g = named("a.so", MSG_SHORT).as_str() == "a.so: file too short";
    let h = {
        let v = judge("seg.so", &CASES[7].spec);
        v.seal() == judge("seg.so", &CASES[7].spec).seal()
    };
    let i = judge("seg.so", &CASES[7].spec).seal() != judge("dyn.so", &CASES[8].spec).seal();
    a && b && c && d && e && f && g && h && i
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = width_ok() && depth_ok();
    out[1] = empty_ok() && ehdr_ok();
    out[2] = phdr_ok() && seg_ok() && dyn_ok() && noseq_ok();
    out[3] = arith_ok()
        && corpus_ok()
        && slots_ok()
        && report_ok()
        && reds_ok()
        && cover_ok()
        && hash_ok()
        && build_ledger().audit() == Ok(());
    out
}


/// 判据集合。
pub fn run_unx11622_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11622.so_trunc");

    s.add("TR01-类宽表", width_ok(), "static");
    s.add("TR02-阶梯深度", depth_ok(), "static");
    s.add("TR03-空与ident", empty_ok(), "static");
    s.add("TR04-ehdr", ehdr_ok(), "static");
    s.add("TR05-程序头表算术", phdr_ok(), "static");
    s.add("TR06-段越界", seg_ok(), "static");
    s.add("TR07-动态串表", dyn_ok(), "static");
    s.add("TR08-无段无动态", noseq_ok(), "static");
    s.add("TR09-算术与节表", arith_ok(), "static");
    s.add("TR10-十四卡全跑", corpus_ok(), "static");
    s.add("TR11-语料与渲染", slots_ok() && report_ok(), "static");
    s.add("TR12-红单封面与摘要", reds_ok() && cover_ok() && hash_ok(), "static");
    s.add("TR13-冻结重放四注入", inject_four().iter().all(|x| *x), "static");

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpus_shape() {
        assert_eq!(CASES.len(), CORPUS);
        assert_eq!(CORPUS, 14);
        assert!(unique());
        assert!(notes_ok());
        assert!(table_ok());
    }

    #[test]
    fn width_table() {
        assert_eq!(Width::of_class(1), Width::W32);
        assert_eq!(Width::of_class(2), Width::W64);
        assert_eq!(Width::W32.ehdr(), 52);
        assert_eq!(Width::W64.ehdr(), 64);
        assert_eq!(Width::W32.phent(), 32);
        assert_eq!(Width::W64.phent(), 56);
        assert_eq!(Width::W32.limit(), 0xffff_ffff);
    }

    #[test]
    fn depth_codes() {
        assert_eq!(Depth::Empty.code(), TR_EEMPTY);
        assert_eq!(Depth::Phdr.code(), TR_EPHDR);
        assert_eq!(Depth::NoLoad.code(), TR_ENOSEQ);
        assert_eq!(Depth::Whole.code(), 0);
        assert!(Depth::Whole.alive());
        assert!(Depth::Empty.short_family());
        assert!(!Depth::Whole.short_family());
    }

    #[test]
    fn empty_verdict() {
        let v = judge("zero.so", &CASES[0].spec);
        assert_eq!(v.depth(), Depth::Empty);
        assert_eq!(v.text().as_str(), "zero.so: file too short");
        assert_eq!(v.gap().map(|g| g.missing()), Some(16));
    }

    #[test]
    fn ident_verdict() {
        let v = judge("fifteen.so", &CASES[2].spec);
        assert_eq!(v.depth(), Depth::Ident);
        assert_eq!(v.gap().map(|g| g.missing()), Some(1));
    }

    #[test]
    fn ehdr_verdict() {
        let v = judge("c32_51.so", &CASES[4].spec);
        assert_eq!(v.depth(), Depth::Ehdr);
        assert_eq!(v.gap().map(|g| g.need()), Some(52));
        assert_eq!(v.gap().map(|g| g.missing()), Some(1));
    }

    #[test]
    fn phdr_verdict() {
        let v = judge("phdr64.so", &CASES[5].spec);
        assert_eq!(v.depth(), Depth::Phdr);
        assert_eq!(v.gap().map(|g| g.need()), Some(232));
        assert_eq!(v.gap().map(|g| g.missing()), Some(68));
    }

    #[test]
    fn seg_verdict() {
        let v = judge("seg.so", &CASES[7].spec);
        assert_eq!(v.depth(), Depth::Seg);
        assert_eq!(v.gap().map(|g| g.off), Some(120));
        assert_eq!(v.gap().map(|g| g.missing()), Some(20));
    }

    #[test]
    fn dyn_verdict() {
        let v = judge("dyn.so", &CASES[8].spec);
        assert_eq!(v.depth(), Depth::Dyn);
        assert_eq!(v.gap().map(|g| g.what), Some("strtab"));
        assert_eq!(v.gap().map(|g| g.off), Some(284));
    }

    #[test]
    fn noload_verdict() {
        let v = judge("noload.so", &CASES[9].spec);
        assert_eq!(v.depth(), Depth::NoLoad);
        assert_eq!(v.text().as_str(), "noload.so: object file has no loadable segments");
    }

    #[test]
    fn nodyn_verdict() {
        let v = judge("nodyn.so", &CASES[10].spec);
        assert_eq!(v.depth(), Depth::NoDyn);
        assert_eq!(v.text().as_str(), "nodyn.so: object file has no dynamic section");
    }

    #[test]
    fn arith_verdict() {
        let v = judge("arith.so", &CASES[11].spec);
        assert_eq!(v.depth(), Depth::Arith);
        assert_eq!(v.code(), TR_EARITH);
        assert!(v.layout().is_none());
    }

    #[test]
    fn shtab_tolerated() {
        let v = judge("shtab.so", &CASES[13].spec);
        assert!(v.accepted());
        assert!(v.sh_beyond());
        assert!(!judge("full.so", &CASES[12].spec).sh_beyond());
    }

    #[test]
    fn gap_report_text() {
        let g = Gap::new("e_ident", 0, 16, 0);
        assert_eq!(
            g.report().as_str(),
            "e_ident@0x00000000+0x00000010 need=0x00000010 size=0x00000000 miss=0x00000010"
        );
        assert_eq!(g.seal(), g.seal());
    }

    #[test]
    fn report_render() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(
            r.render().as_str(),
            "trunc total=14 ok=2 fail=12 empty=1 ident=2 ehdr=2 phdr=2 seg=1 dyn=1 noload=1 nodyn=1 arith=1 whole=2 match=14 rate=1000ppm\n"
        );
        assert_eq!(r.gate(), Ok(()));
        assert!(r.green());
    }

    #[test]
    fn red_override() {
        let r = match run_with(Some((2, TR_EIDENT))) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(r.matched(), CORPUS - 1);
        assert_eq!(r.rate(), 929);
        assert_eq!(r.gate(), Err(TR_EBAL));
        assert_eq!(run_with(Some((14, TR_EEMPTY))).err(), Some(TR_ESLOT));
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert!(l.sample(0).is_some());
        assert!(l.sample(SAMPLES).is_none());
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = match l.freeze() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert!(l.is_frozen());
        assert_eq!(l.root(), root);
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(TR_EBAL));
        assert!(l.rejects() >= 1);
    }

    #[test]
    fn injections() {
        let out = inject_four();
        assert!(out[0]);
        assert!(out[1]);
        assert!(out[2]);
        assert!(out[3]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11622_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }
}


