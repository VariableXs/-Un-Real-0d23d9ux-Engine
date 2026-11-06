//! UNX-F11632 TEXTREL 只读段重定位策略判据（musl 拒绝/glibc 允许双线）
//!
//! 闸门次序：旗标一致 → 重定位类型 → 绑定模式 → 行策略 → mprotect → 补槽 → 恢复保护。
//! 契约：纯 no_std 风格（仅 `alloc::string::String` 渲染），无 unsafe、无 unwrap/expect/panic!，失败一律 `Result<_, u32>`，域码 + 状态码恰 13 个。
//!
//! `DT_TEXTREL`（旧式动态项）或 `DF_TEXTREL`（`DT_FLAGS` 位）表示对象里存在
//! **落在只读段上的重定位**。同一份 ELF，两条装载器路线给出相反结论：
//!
//! - glibc：允许。`_dl_relocate_object` 发现文本重定位后把目标 LOAD 段临时
//!   `mprotect(PROT_READ|PROT_WRITE|PROT_EXEC)`，逐条写槽，再恢复
//!   `PROT_READ|PROT_EXEC`；失败才拒载（`TX_EGLIB` / `TX_EMPROT`）。
//! - musl：拒绝。装载器把文本重定位当成不可支持的输入，直接以
//!   `"<path>: text relocations"` 一类错误终止（`TX_EMUSL`）。
//!
//! 判据因此必须是**双线**的：同一组输入在 glibc 线要放行并验证"临时可写、
//! 事后只读可执行"，在 musl 线要拒收；不能把某条线的策略当成唯一真理。
//! 除了两条路线差异，还有四类与路线无关的坏输入：
//!
//! - 有文本重定位却没有 `DT_TEXTREL`/`DF_TEXTREL` 旗标（`TX_EFLAGS`）；
//! - 文本重定位类型本实现不认识（`TX_ERELOC`）；
//! - 文本重定位 + 延迟绑定声明，触发点无法在装载期收敛（`TX_EBIND`）；
//! - glibc 线 `mprotect` 失败（`TX_EGLIB`）或恢复只读失败（`TX_EMPROT`）。
//!
//! 14 槽语料 8 活 6 死：活档覆盖双线洁净、musl 数据槽、glibc 单条/双条/弱符号/
//! RELRO 交叉、以及"旗标冗余但全是数据槽"的容忍档；死档对应上面六类。所有
//! 活档在装载完成后文本段必须回到 `r-x`，这是本卡与"能跑就行"的分界线。
//!
//! 判据面、渲染面与账本面共用同一份语料；任何一面漂移都会让
//! `run_unx11632_checks()` 变红。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfe50;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 文本重定位容量。
pub const RELOC_CAP: usize = 4;

/// 轨迹容量。
pub const TRACE_CAP: usize = 10;

/// `PROT_READ`。
pub const PROT_READ: u8 = 0x1;

/// `PROT_WRITE`。
pub const PROT_WRITE: u8 = 0x2;

/// `PROT_EXEC`。
pub const PROT_EXEC: u8 = 0x4;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// glibc 线 mprotect 失败。
pub const TX_EGLIB: u32 = SEG_BASE + 0x01;
/// musl 线拒绝文本重定位。
pub const TX_EMUSL: u32 = SEG_BASE + 0x02;
/// 恢复只读可执行失败。
pub const TX_EMPROT: u32 = SEG_BASE + 0x03;
/// TEXTREL 旗标与重定位不一致。
pub const TX_EFLAGS: u32 = SEG_BASE + 0x04;
/// 文本重定位类型不支持。
pub const TX_ERELOC: u32 = SEG_BASE + 0x05;
/// 文本重定位与绑定模式冲突。
pub const TX_EBIND: u32 = SEG_BASE + 0x06;
/// 账本状态错。
pub const TX_ESTATE: u32 = SEG_BASE + 0x07;
/// 期望值不符。
pub const TX_EVERIFY: u32 = SEG_BASE + 0x08;
/// 对账不平。
pub const TX_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位越界。
pub const TX_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与常量表不符。
pub const TX_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结指纹失配。
pub const TX_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无可观测样本。
pub const TX_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 语料汇总。
pub const OP_CORPUS: u8 = 1;
/// 旗标。
pub const OP_FLAGS: u8 = 2;
/// 重定位。
pub const OP_RELOC: u8 = 3;
/// 保护。
pub const OP_PROT: u8 = 4;
/// 双线。
pub const OP_LINE: u8 = 5;
/// 文案。
pub const OP_TEXT: u8 = 6;

// ---------------------------------------------------------------------------
// FNV-1a
// ---------------------------------------------------------------------------

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 折叠一个字节。
fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(FNV_PRIME)
}

/// 折叠一个字（大端 8 字节）。
fn fold_word(h: u64, w: u64) -> u64 {
    let mut x = h;
    let mut i = 0usize;
    while i < 8 {
        let shift = (7 - i) * 8;
        let b = ((w >> shift) & 0xff) as u8;
        x = fold_byte(x, b);
        i += 1;
    }
    x
}

/// 计算字节串摘要。
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

/// 十进制文本。
pub fn u32_text(v: u32) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 10];
    let mut i = buf.len();
    let mut n = v;
    while n > 0 {
        let d = (n % 10) as u8;
        i -= 1;
        if let Some(slot) = buf.get_mut(i) {
            *slot = b'0' + d;
        }
        n /= 10;
    }
    let mut s = String::new();
    while i < buf.len() {
        if let Some(b) = buf.get(i) {
            s.push(*b as char);
        }
        i += 1;
    }
    s
}

/// 固定八位十六进制文本。
pub fn hex_text(v: u32) -> String {
    const HEX: [u8; 16] = [
        b'0', b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', b'a', b'b', b'c', b'd', b'e',
        b'f',
    ];
    let mut s = String::from("0x");
    let mut i = 0usize;
    while i < 8 {
        let shift = (7 - i) * 4;
        let nib = ((v >> shift) & 0xf) as usize;
        match HEX.get(nib) {
            Some(b) => s.push(*b as char),
            None => s.push('0'),
        }
        i += 1;
    }
    s
}

// ---------------------------------------------------------------------------
// 保护位
// ---------------------------------------------------------------------------

/// 段保护位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Prot {
    /// 可读。
    pub r: bool,
    /// 可写。
    pub w: bool,
    /// 可执行。
    pub x: bool,
}

impl Prot {
    /// 只读可执行（文本段常态）。
    pub const RX: Prot = Prot { r: true, w: false, x: true };
    /// 只读可写（mprotect 窗口）。
    pub const RWX: Prot = Prot { r: true, w: true, x: true };
    /// 只读数据。
    pub const R: Prot = Prot { r: true, w: false, x: false };
    /// 可读写数据。
    pub const RW: Prot = Prot { r: true, w: true, x: false };

    /// 位编码。
    pub const fn bits(self) -> u8 {
        let mut b = 0u8;
        if self.r {
            b |= PROT_READ;
        }
        if self.w {
            b |= PROT_WRITE;
        }
        if self.x {
            b |= PROT_EXEC;
        }
        b
    }

    /// 标签。
    pub const fn label(self) -> &'static str {
        if self.r && self.w && self.x {
            "rwx"
        } else if self.r && self.w {
            "rw-"
        } else if self.r && self.x {
            "r-x"
        } else if self.r {
            "r--"
        } else {
            "---"
        }
    }

    /// 是否文本保护。
    pub const fn is_text(self) -> bool {
        self.r && !self.w && self.x
    }

    /// 是否可写。
    pub const fn writable(self) -> bool {
        self.w
    }
}

// ---------------------------------------------------------------------------
// 装载路线
// ---------------------------------------------------------------------------

/// 装载器路线。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line {
    /// glibc：允许文本重定位，临时可写。
    Glibc,
    /// musl：拒绝文本重定位。
    Musl,
}

impl Line {
    /// 标签。
    pub const fn label(self) -> &'static str {
        match self {
            Line::Glibc => "glibc",
            Line::Musl => "musl",
        }
    }

    /// 是否允许文本重定位。
    pub const fn allows_textrel(self) -> bool {
        match self {
            Line::Glibc => true,
            Line::Musl => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 重定位
// ---------------------------------------------------------------------------

/// 文本重定位种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelocKind {
    /// `R_X86_64_64`。
    Abs64,
    /// `R_X86_64_32`。
    Abs32,
    /// `R_X86_64_PC32`。
    Pc32,
    /// 本实现不认识。
    Unknown,
}

impl RelocKind {
    /// 编码。
    pub const fn code(self) -> u32 {
        match self {
            RelocKind::Abs64 => 1,
            RelocKind::Abs32 => 10,
            RelocKind::Pc32 => 2,
            RelocKind::Unknown => 99,
        }
    }

    /// 标签。
    pub const fn label(self) -> &'static str {
        match self {
            RelocKind::Abs64 => "abs64",
            RelocKind::Abs32 => "abs32",
            RelocKind::Pc32 => "pc32",
            RelocKind::Unknown => "unknown",
        }
    }

    /// 是否支持。
    pub const fn known(self) -> bool {
        match self {
            RelocKind::Unknown => false,
            _ => true,
        }
    }
}

/// 一条重定位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextReloc {
    /// 段内偏移。
    pub offset: u32,
    /// 种类。
    pub kind: RelocKind,
    /// 目标符号。
    pub target: &'static str,
    /// 是否弱符号。
    pub weak: bool,
    /// 是否落在文本段。
    pub in_text: bool,
}

impl TextReloc {
    /// 文本段重定位。
    pub const fn text(offset: u32, kind: RelocKind, target: &'static str) -> TextReloc {
        TextReloc { offset, kind, target, weak: false, in_text: true }
    }

    /// 弱符号文本重定位。
    pub const fn weak_text(offset: u32, kind: RelocKind, target: &'static str) -> TextReloc {
        TextReloc { offset, kind, target, weak: true, in_text: true }
    }

    /// 数据段重定位。
    pub const fn data(offset: u32, kind: RelocKind, target: &'static str) -> TextReloc {
        TextReloc { offset, kind, target, weak: false, in_text: false }
    }

    /// 是否有效。
    pub const fn valid(&self) -> bool {
        self.kind.known() && self.offset < 0x0010_0000
    }
}

// ---------------------------------------------------------------------------
// 镜像
// ---------------------------------------------------------------------------

/// 一个带文本重定位的装载镜像。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Image {
    /// 名字。
    pub name: &'static str,
    /// 路线。
    pub line: Line,
    /// 文本段保护。
    pub text: Prot,
    /// 数据段保护。
    pub data: Prot,
    /// 重定位。
    pub relocs: [TextReloc; RELOC_CAP],
    /// 重定位条数。
    pub reloc_len: u8,
    /// `DT_TEXTREL`。
    pub dt_textrel: bool,
    /// `DF_TEXTREL`。
    pub df_textrel: bool,
    /// 部分 RELRO。
    pub relro: bool,
    /// 声明延迟绑定。
    pub lazy_bind: bool,
    /// mprotect 是否成功。
    pub mprotect_ok: bool,
    /// 恢复只读是否成功。
    pub restore_ok: bool,
}

impl Image {
    /// 空镜像。
    pub fn blank(name: &'static str, line: Line) -> Image {
        Image {
            name,
            line,
            text: Prot::RX,
            data: Prot::RW,
            relocs: [TextReloc::data(0, RelocKind::Abs64, ""); RELOC_CAP],
            reloc_len: 0,
            dt_textrel: false,
            df_textrel: false,
            relro: false,
            lazy_bind: false,
            mprotect_ok: true,
            restore_ok: true,
        }
    }

    /// 追加重定位。
    pub fn push(&mut self, r: TextReloc) -> bool {
        if (self.reloc_len as usize) >= RELOC_CAP {
            return false;
        }
        if let Some(dst) = self.relocs.get_mut(self.reloc_len as usize) {
            *dst = r;
        }
        self.reloc_len += 1;
        true
    }

    /// 重定位条数。
    pub const fn len(&self) -> usize {
        self.reloc_len as usize
    }

    /// 取重定位。
    pub fn reloc_at(&self, i: usize) -> Option<TextReloc> {
        if i >= self.reloc_len as usize {
            return None;
        }
        self.relocs.get(i).copied()
    }

    /// 文本重定位数。
    pub fn text_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.reloc_len as usize {
            if let Some(r) = self.relocs.get(i) {
                if r.in_text {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 数据重定位数。
    pub fn data_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.reloc_len as usize {
            if let Some(r) = self.relocs.get(i) {
                if !r.in_text {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 弱文本重定位数。
    pub fn weak_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.reloc_len as usize {
            if let Some(r) = self.relocs.get(i) {
                if r.in_text && r.weak {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 是否有不支持的文本重定位。
    pub fn bad_reloc(&self) -> bool {
        let mut i = 0usize;
        while i < self.reloc_len as usize {
            if let Some(r) = self.relocs.get(i) {
                if r.in_text && !r.kind.known() {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 文本重定位旗标。
    pub const fn textrel_flag(&self) -> bool {
        self.dt_textrel || self.df_textrel
    }
}

// ---------------------------------------------------------------------------
// 轨迹
// ---------------------------------------------------------------------------

/// 阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 装载。
    Load,
    /// 旗标。
    Flags,
    /// 重定位类型。
    Reloc,
    /// 绑定模式。
    Bind,
    /// 临时放开写。
    Mprotect,
    /// 写槽。
    Apply,
    /// 恢复保护。
    Restore,
    /// 完成。
    Done,
}

impl Stage {
    /// 标签。
    pub const fn label(self) -> &'static str {
        match self {
            Stage::Load => "load",
            Stage::Flags => "flags",
            Stage::Reloc => "reloc",
            Stage::Bind => "bind",
            Stage::Mprotect => "mprotect",
            Stage::Apply => "apply",
            Stage::Restore => "restore",
            Stage::Done => "done",
        }
    }
}

/// 轨迹中的一个注记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Note {
    /// 阶段。
    pub stage: Stage,
    /// 成败。
    pub ok: bool,
    /// 值。
    pub value: u32,
}

impl Note {
    /// 空格。
    pub const fn blank() -> Note {
        Note { stage: Stage::Load, ok: false, value: 0 }
    }
}

/// 轨迹。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trace {
    /// 注记。
    pub notes: [Note; TRACE_CAP],
    /// 注记数。
    pub len: u8,
}

impl Trace {
    /// 空轨迹。
    pub const fn blank() -> Trace {
        Trace { notes: [Note::blank(); TRACE_CAP], len: 0 }
    }

    /// 追加注记。
    pub fn note(&mut self, stage: Stage, ok: bool, value: u32) {
        if (self.len as usize) >= TRACE_CAP {
            return;
        }
        if let Some(dst) = self.notes.get_mut(self.len as usize) {
            *dst = Note { stage, ok, value };
        }
        self.len += 1;
    }

    /// 注记数。
    pub const fn len(&self) -> usize {
        self.len as usize
    }

    /// 取注记。
    pub fn at(&self, i: usize) -> Option<(Stage, bool, u32)> {
        if i >= self.len as usize {
            return None;
        }
        match self.notes.get(i) {
            Some(n) => Some((n.stage, n.ok, n.value)),
            None => None,
        }
    }

    /// 最后一条。
    pub fn last(&self) -> Option<(Stage, bool, u32)> {
        if self.len == 0 {
            return None;
        }
        self.at(self.len as usize - 1)
    }

    /// 是否含某阶段。
    pub fn has(&self, stage: Stage) -> bool {
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(n) = self.notes.get(i) {
                if n.stage == stage {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 某阶段次数。
    pub fn count(&self, stage: Stage) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(x) = self.notes.get(i) {
                if x.stage == stage {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 判面输出
// ---------------------------------------------------------------------------

/// 一次装载判面的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verdict {
    /// 名字。
    pub name: &'static str,
    /// 路线。
    pub line: Line,
    /// 码；0 表示放行。
    pub code: u32,
    /// 文本重定位补槽数。
    pub patched: u8,
    /// 其中弱符号数。
    pub weak_patched: u8,
    /// 数据重定位补槽数。
    pub data_patched: u8,
    /// mprotect 调用数。
    pub mprotect_calls: u8,
    /// 装载完成后的文本保护。
    pub text: Prot,
    /// 是否给出警告（glibc 对文本重定位/冗余旗标发警告）。
    pub warning: bool,
    /// 轨迹。
    pub trace: Trace,
}

impl Verdict {
    /// 空。
    pub const fn blank() -> Verdict {
        Verdict {
            name: "",
            line: Line::Glibc,
            code: 0,
            patched: 0,
            weak_patched: 0,
            data_patched: 0,
            mprotect_calls: 0,
            text: Prot::RX,
            warning: false,
            trace: Trace::blank(),
        }
    }

    /// 放行。
    pub const fn passed(&self) -> bool {
        self.code == 0
    }

    /// 码。
    pub const fn code(&self) -> u32 {
        self.code
    }

    /// 文本段是否回到只读可执行。
    pub const fn restored(&self) -> bool {
        self.text.is_text()
    }

    /// 文案。
    pub fn text(&self) -> String {
        let mut s = String::new();
        if self.passed() {
            s.push_str("textrel-ok ");
        } else {
            s.push_str("textrel-reject ");
        }
        s.push_str(self.name);
        s.push_str(" line=");
        s.push_str(self.line.label());
        s.push_str(" patched=");
        s.push_str(u32_text(self.patched as u32).as_str());
        s.push_str(" prot=");
        s.push_str(self.text.label());
        if self.passed() {
            s.push_str(" warn=");
            s.push_str(u32_text(self.warning as u32).as_str());
        } else {
            s.push_str(" code=");
            s.push_str(hex_text(self.code).as_str());
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 装载判面
// ---------------------------------------------------------------------------

/// 拒收判面。
fn reject(img: &Image, code: u32, tr: Trace) -> Verdict {
    Verdict {
        name: img.name,
        line: img.line,
        code,
        patched: 0,
        weak_patched: 0,
        data_patched: 0,
        mprotect_calls: 0,
        text: img.text,
        warning: false,
        trace: tr,
    }
}

/// 放行判面。
fn accept(
    img: &Image,
    patched: u8,
    weak_patched: u8,
    mprotect_calls: u8,
    text: Prot,
    warning: bool,
    tr: Trace,
) -> Verdict {
    Verdict {
        name: img.name,
        line: img.line,
        code: 0,
        patched,
        weak_patched,
        data_patched: img.data_count() as u8,
        mprotect_calls,
        text,
        warning,
        trace: tr,
    }
}

/// 装载一个镜像。
pub fn load(img: &Image) -> Verdict {
    let mut tr = Trace::blank();
    tr.note(Stage::Load, true, img.reloc_len as u32);

    let textrels = img.text_count();
    let flag = img.textrel_flag();

    // 1. 旗标一致性：有文本重定位就必须有旗标。
    if textrels > 0 && !flag {
        tr.note(Stage::Flags, false, textrels as u32);
        return reject(img, TX_EFLAGS, tr);
    }
    tr.note(Stage::Flags, true, if flag { 1 } else { 0 });

    // 2. 类型：只检查落在文本段的重定位。
    if img.bad_reloc() {
        tr.note(Stage::Reloc, false, textrels as u32);
        return reject(img, TX_ERELOC, tr);
    }
    tr.note(Stage::Reloc, true, textrels as u32);

    // 3. 绑定模式：文本重定位必须在装载期收敛。
    if textrels > 0 && img.lazy_bind {
        tr.note(Stage::Bind, false, 1);
        return reject(img, TX_EBIND, tr);
    }
    tr.note(Stage::Bind, true, 0);

    // 4. 路线策略。
    match img.line {
        Line::Musl => {
            if flag || textrels > 0 {
                tr.note(Stage::Mprotect, false, 0);
                return reject(img, TX_EMUSL, tr);
            }
            tr.note(Stage::Apply, true, 0);
            tr.note(Stage::Done, true, 0);
            accept(img, 0, 0, 0, img.text, false, tr)
        }
        Line::Glibc => {
            if textrels == 0 {
                // 旗标冗余但全是数据槽：glibc 容忍，只发警告。
                tr.note(Stage::Apply, true, 0);
                tr.note(Stage::Done, true, 0);
                accept(img, 0, 0, 0, img.text, flag, tr)
            } else if !img.mprotect_ok {
                tr.note(Stage::Mprotect, false, 1);
                return reject(img, TX_EGLIB, tr);
            } else {
                tr.note(Stage::Mprotect, true, 1);
                // 临时 RWX 窗口。
                tr.note(Stage::Apply, true, textrels as u32);
                if !img.restore_ok {
                    tr.note(Stage::Restore, false, Prot::RWX.bits() as u32);
                    // 恢复失败：文本段实际留在可写可执行状态。
                    let mut left = *img;
                    left.text = Prot::RWX;
                    let mut v = reject(&left, TX_EMPROT, tr);
                    v.mprotect_calls = 1;
                    return v;
                }
                tr.note(Stage::Restore, true, Prot::RX.bits() as u32);
                tr.note(Stage::Done, true, 0);
                accept(
                    img,
                    textrels as u8,
                    img.weak_count() as u8,
                    1,
                    Prot::RX,
                    true,
                    tr,
                )
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 场景。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    /// glibc 洁净对象。
    CleanGlibc,
    /// musl 洁净对象。
    CleanMusl,
    /// musl 纯数据槽。
    MuslData,
    /// glibc 单条文本重定位。
    SingleText,
    /// glibc 双条文本重定位。
    PairText,
    /// glibc 弱符号文本重定位。
    WeakText,
    /// glibc RELRO 交叉。
    RelroText,
    /// glibc 冗余旗标但全是数据槽。
    DataFlag,
    /// musl 遇文本重定位。
    MuslText,
    /// glibc mprotect 失败。
    MprotectFail,
    /// glibc 恢复只读失败。
    RestoreFail,
    /// 缺 TEXTREL 旗标。
    FlagMissing,
    /// 文本重定位类型不支持。
    RelocBad,
    /// 延迟绑定与文本重定位冲突。
    LazyTextrel,
}

impl Scenario {
    /// 标签。
    pub const fn label(self) -> &'static str {
        match self {
            Scenario::CleanGlibc => "clean-glibc",
            Scenario::CleanMusl => "clean-musl",
            Scenario::MuslData => "musl-data",
            Scenario::SingleText => "single-text",
            Scenario::PairText => "pair-text",
            Scenario::WeakText => "weak-text",
            Scenario::RelroText => "relro-text",
            Scenario::DataFlag => "data-flag",
            Scenario::MuslText => "musl-text",
            Scenario::MprotectFail => "mprotect-fail",
            Scenario::RestoreFail => "restore-fail",
            Scenario::FlagMissing => "flag-missing",
            Scenario::RelocBad => "reloc-bad",
            Scenario::LazyTextrel => "lazy-textrel",
        }
    }
}

/// 一条语料。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    /// 名字。
    pub name: &'static str,
    /// 场景。
    pub scenario: Scenario,
    /// 期望码。
    pub expect: u32,
    /// 说明。
    pub note: &'static str,
}

impl Case {
    /// 是否放行档。
    pub const fn accept(&self) -> bool {
        self.expect == 0
    }

    /// 输入键。
    pub fn input_key(&self) -> u64 {
        let mut h = digest(self.name.as_bytes());
        h = fold_word(h, self.scenario as u64);
        h
    }

    /// 说明键。
    pub fn note_key(&self) -> u64 {
        digest(self.note.as_bytes())
    }
}

/// 14 槽：8 活 6 死。
pub const CASES: [Case; CORPUS] = [
    Case {
        name: "clean",
        scenario: Scenario::CleanGlibc,
        expect: 0,
        note: "glibc 洁净对象，无旗标无文本重定位，文本段保持 r-x",
    },
    Case {
        name: "cleanmusl",
        scenario: Scenario::CleanMusl,
        expect: 0,
        note: "同一洁净输入走 musl 线也放行，说明拒的是 TEXTREL 不是 ELF",
    },
    Case {
        name: "musldata",
        scenario: Scenario::MuslData,
        expect: 0,
        note: "musl 对象只有数据槽重定位，不触发文本重定位拒收",
    },
    Case {
        name: "single",
        scenario: Scenario::SingleText,
        expect: 0,
        note: "glibc 单条文本重定位：临时 RWX、补一条、恢复 r-x",
    },
    Case {
        name: "pair",
        scenario: Scenario::PairText,
        expect: 0,
        note: "glibc 双条文本重定位共用一次 mprotect 窗口",
    },
    Case {
        name: "weak",
        scenario: Scenario::WeakText,
        expect: 0,
        note: "弱符号文本重定位照常补槽，不因弱绑定跳过",
    },
    Case {
        name: "relro",
        scenario: Scenario::RelroText,
        expect: 0,
        note: "部分 RELRO 与文本重定位并存，glibc 仍允许并恢复保护",
    },
    Case {
        name: "dataflag",
        scenario: Scenario::DataFlag,
        expect: 0,
        note: "TEXTREL 旗标冗余但重定位全在数据段，glibc 只警告不放拒",
    },
    Case {
        name: "musltext",
        scenario: Scenario::MuslText,
        expect: TX_EMUSL,
        note: "musl 线看到文本重定位直接拒，这是双线判据的对照点",
    },
    Case {
        name: "mprotectfail",
        scenario: Scenario::MprotectFail,
        expect: TX_EGLIB,
        note: "glibc 线 mprotect 放开写失败，无法补槽只能拒载",
    },
    Case {
        name: "restorefail",
        scenario: Scenario::RestoreFail,
        expect: TX_EMPROT,
        note: "glibc 补槽后恢复只读失败，文本段留在 RWX，判拒",
    },
    Case {
        name: "flagmissing",
        scenario: Scenario::FlagMissing,
        expect: TX_EFLAGS,
        note: "存在文本重定位却无 DT_TEXTREL/DF_TEXTREL，旗标不一致",
    },
    Case {
        name: "relocbad",
        scenario: Scenario::RelocBad,
        expect: TX_ERELOC,
        note: "文本段上的重定位类型 99 本实现不认识，拒",
    },
    Case {
        name: "lazytextrel",
        scenario: Scenario::LazyTextrel,
        expect: TX_EBIND,
        note: "文本重定位声明延迟绑定，触发点无法在装载期收敛，拒",
    },
];

/// 取槽。
pub fn slot_at(i: usize) -> Option<Case> {
    if i >= CORPUS {
        return None;
    }
    CASES.get(i).copied()
}

/// 造镜像。
pub fn build(sc: Scenario) -> Image {
    match sc {
        Scenario::CleanGlibc => Image::blank("./libtext.so", Line::Glibc),
        Scenario::CleanMusl => Image::blank("./libtext.so", Line::Musl),
        Scenario::MuslData => {
            let mut img = Image::blank("./libtext.so", Line::Musl);
            let _ = img.push(TextReloc::data(0x2000, RelocKind::Abs64, "stderr"));
            img
        }
        Scenario::SingleText => {
            let mut img = Image::blank("./libtext.so", Line::Glibc);
            let _ = img.push(TextReloc::text(0x1000, RelocKind::Abs64, "foo"));
            img.dt_textrel = true;
            img.df_textrel = true;
            img
        }
        Scenario::PairText => {
            let mut img = Image::blank("./libtext.so", Line::Glibc);
            let _ = img.push(TextReloc::text(0x1000, RelocKind::Abs64, "foo"));
            let _ = img.push(TextReloc::text(0x1010, RelocKind::Abs32, "bar"));
            img.dt_textrel = true;
            img.df_textrel = true;
            img
        }
        Scenario::WeakText => {
            let mut img = Image::blank("./libtext.so", Line::Glibc);
            let _ = img.push(TextReloc::weak_text(0x1020, RelocKind::Abs64, "maybe"));
            img.dt_textrel = true;
            img
        }
        Scenario::RelroText => {
            let mut img = Image::blank("./libtext.so", Line::Glibc);
            let _ = img.push(TextReloc::text(0x1030, RelocKind::Pc32, "foo"));
            img.dt_textrel = true;
            img.df_textrel = true;
            img.relro = true;
            img
        }
        Scenario::DataFlag => {
            let mut img = Image::blank("./libtext.so", Line::Glibc);
            let _ = img.push(TextReloc::data(0x2000, RelocKind::Abs64, "stderr"));
            img.dt_textrel = true;
            img.df_textrel = true;
            img
        }
        Scenario::MuslText => {
            let mut img = Image::blank("./libtext.so", Line::Musl);
            let _ = img.push(TextReloc::text(0x1000, RelocKind::Abs64, "foo"));
            img.dt_textrel = true;
            img.df_textrel = true;
            img
        }
        Scenario::MprotectFail => {
            let mut img = Image::blank("./libtext.so", Line::Glibc);
            let _ = img.push(TextReloc::text(0x1000, RelocKind::Abs64, "foo"));
            img.dt_textrel = true;
            img.df_textrel = true;
            img.mprotect_ok = false;
            img
        }
        Scenario::RestoreFail => {
            let mut img = Image::blank("./libtext.so", Line::Glibc);
            let _ = img.push(TextReloc::text(0x1000, RelocKind::Abs64, "foo"));
            img.dt_textrel = true;
            img.df_textrel = true;
            img.restore_ok = false;
            img
        }
        Scenario::FlagMissing => {
            let mut img = Image::blank("./libtext.so", Line::Glibc);
            let _ = img.push(TextReloc::text(0x1000, RelocKind::Abs64, "foo"));
            img
        }
        Scenario::RelocBad => {
            let mut img = Image::blank("./libtext.so", Line::Glibc);
            let _ = img.push(TextReloc::text(0x1000, RelocKind::Unknown, "foo"));
            img.dt_textrel = true;
            img.df_textrel = true;
            img
        }
        Scenario::LazyTextrel => {
            let mut img = Image::blank("./libtext.so", Line::Glibc);
            let _ = img.push(TextReloc::text(0x1000, RelocKind::Abs64, "foo"));
            img.dt_textrel = true;
            img.df_textrel = true;
            img.lazy_bind = true;
            img
        }
    }
}

/// 求解。
pub fn solve(c: &Case) -> Verdict {
    load(&build(c.scenario))
}

/// 唯一性。
pub fn unique() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        let a = match CASES.get(i) {
            Some(v) => v,
            None => return false,
        };
        let mut j = i + 1;
        while j < CASES.len() {
            let b = match CASES.get(j) {
                Some(v) => v,
                None => return false,
            };
            if a.name == b.name || a.note == b.note || a.scenario == b.scenario {
                return false;
            }
            if a.input_key() == b.input_key() || a.note_key() == b.note_key() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 说明自洽。
pub fn notes_ok() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        let c = match CASES.get(i) {
            Some(v) => v,
            None => return false,
        };
        if c.name.is_empty() || c.note.is_empty() || c.scenario.label().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

/// 逐槽对齐。
pub fn table_ok() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        let c = match CASES.get(i) {
            Some(v) => v,
            None => return false,
        };
        let j = solve(&c);
        if j.code() != c.expect {
            return false;
        }
        if c.accept() != j.passed() {
            return false;
        }
        if j.passed() && !j.restored() {
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    total: usize,
    accept: usize,
    reject: usize,
    glibc: usize,
    musl: usize,
    mprotect: usize,
    flags: usize,
    matched: usize,
    red: bool,
}

impl Report {
    /// 空。
    pub const fn new() -> Report {
        Report {
            total: 0,
            accept: 0,
            reject: 0,
            glibc: 0,
            musl: 0,
            mprotect: 0,
            flags: 0,
            matched: 0,
            red: false,
        }
    }

    /// 总数。
    pub const fn total(&self) -> usize {
        self.total
    }

    /// 接受。
    pub const fn accept(&self) -> usize {
        self.accept
    }

    /// 拒收。
    pub const fn reject(&self) -> usize {
        self.reject
    }

    /// glibc 侧拒收桶。
    pub const fn glibc(&self) -> usize {
        self.glibc
    }

    /// musl 侧拒收桶。
    pub const fn musl(&self) -> usize {
        self.musl
    }

    /// 保护恢复桶。
    pub const fn mprotect(&self) -> usize {
        self.mprotect
    }

    /// 旗标/类型/绑定桶。
    pub const fn flags(&self) -> usize {
        self.flags
    }

    /// 对数。
    pub const fn matched(&self) -> usize {
        self.matched
    }

    /// 红档。
    pub const fn red(&self) -> bool {
        self.red
    }

    /// 空。
    pub const fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// 千分率。
    pub fn rate(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        ((self.matched as u32) * 1000 + (self.total as u32) / 2) / (self.total as u32)
    }

    /// 闸门。
    pub fn gate(&self) -> Result<(), u32> {
        if self.total == 0 {
            return Err(TX_ENOBS);
        }
        if self.total != CORPUS {
            return Err(TX_EVALUE);
        }
        if self.accept != 8 || self.reject != 6 {
            return Err(TX_EBAL);
        }
        if self.glibc + self.musl + self.mprotect + self.flags != self.reject {
            return Err(TX_EBAL);
        }
        if self.red {
            return Err(TX_ESTATE);
        }
        if self.matched != self.total {
            return Err(TX_EVERIFY);
        }
        Ok(())
    }

    /// 绿。
    pub fn green(&self) -> bool {
        match self.gate() {
            Ok(()) => true,
            Err(_) => false,
        }
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("textrel corpus=");
        s.push_str(u32_text(self.total as u32).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept as u32).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject as u32).as_str());
        s.push_str(" glibc=");
        s.push_str(u32_text(self.glibc as u32).as_str());
        s.push_str(" musl=");
        s.push_str(u32_text(self.musl as u32).as_str());
        s.push_str(" mprotect=");
        s.push_str(u32_text(self.mprotect as u32).as_str());
        s.push_str(" flags=");
        s.push_str(u32_text(self.flags as u32).as_str());
        s.push_str(" match=");
        s.push_str(u32_text(self.matched as u32).as_str());
        s.push_str(" rate=");
        s.push_str(u32_text(self.rate()).as_str());
        s.push_str("ppm\n");
        s
    }
}

/// 跑。
pub fn run() -> Result<Report, u32> {
    walk(None)
}

/// 跑并注入。
pub fn run_with(red: Option<(usize, u32)>) -> Result<Report, u32> {
    walk(red)
}

/// 行走。
fn walk(red: Option<(usize, u32)>) -> Result<Report, u32> {
    let mut r = Report::new();
    let mut i = 0usize;
    while i < CASES.len() {
        let c = match CASES.get(i) {
            Some(v) => *v,
            None => return Err(TX_EVALUE),
        };
        let j = solve(&c);
        r.total += 1;
        if j.passed() {
            r.accept += 1;
        } else {
            r.reject += 1;
            match j.code() {
                TX_EGLIB => r.glibc += 1,
                TX_EMUSL => r.musl += 1,
                TX_EMPROT => r.mprotect += 1,
                TX_EFLAGS | TX_ERELOC | TX_EBIND => r.flags += 1,
                _ => return Err(TX_EVERIFY),
            }
        }
        if j.code() == c.expect {
            r.matched += 1;
        }
        i += 1;
    }
    if let Some((slot, code)) = red {
        if slot >= r.total {
            return Err(TX_ESLOT);
        }
        if code == 0 {
            return Err(TX_EVALUE);
        }
        r.matched = r.matched.saturating_sub(1);
        r.red = true;
    }
    Ok(r)
}

/// 前缀封面。
pub fn cover(n: usize) -> Result<(), u32> {
    if n > CORPUS {
        return Err(TX_ESLOT);
    }
    let mut i = 0usize;
    while i < n {
        let c = match slot_at(i) {
            Some(v) => v,
            None => return Err(TX_EVALUE),
        };
        if solve(&c).code() != c.expect {
            return Err(TX_EVALUE);
        }
        i += 1;
    }
    Ok(())
}

/// 槽码。
fn case_code(i: usize) -> u32 {
    match slot_at(i) {
        Some(c) => solve(&c).code(),
        None => TX_EVALUE,
    }
}

// ---------------------------------------------------------------------------
// 账本
// ---------------------------------------------------------------------------

/// 一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 槽。
    pub slot: u8,
    /// 操作码。
    pub op: u8,
    /// 成败。
    pub ok: bool,
    /// 码。
    pub code: u32,
    /// 值。
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
    /// 空。
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
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(TX_ESEAL);
        }
        if (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(TX_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 空。
    pub const fn is_empty(&self) -> bool {
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
            return Err(TX_EBAL);
        }
        const VALS: [u32; 8] = [14, 8, 6, 1, 1, 1, 3, 929];
        const CODES: [u32; 6] =
            [TX_EMUSL, TX_EGLIB, TX_EMPROT, TX_EFLAGS, TX_ERELOC, TX_EBIND];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(TX_EBAL),
            };
            if s.slot as usize != i {
                return Err(TX_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(TX_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(TX_EVALUE);
                        }
                    }
                    None => return Err(TX_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(TX_EVALUE);
                        }
                    }
                    None => return Err(TX_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(TX_EBAL);
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
            return Err(TX_ESTATE);
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
            Err(TX_ESEAL)
        }
    }

    /// 指纹。
    pub const fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub const fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝数。
    pub const fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 计数格。
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

/// 建账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();
    let rr = run_with(Some((0, TX_EMUSL)));
    let _ = l.register(count_cell(0, OP_CORPUS, r.map(|x| x.total())));
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.accept())));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.reject())));
    let _ = l.register(count_cell(3, OP_LINE, r.map(|x| x.glibc())));
    let _ = l.register(count_cell(4, OP_LINE, r.map(|x| x.musl())));
    let _ = l.register(count_cell(5, OP_PROT, r.map(|x| x.mprotect())));
    let _ = l.register(count_cell(6, OP_FLAGS, r.map(|x| x.flags())));
    let _ = l.register(count_cell(7, OP_TEXT, rr.map(|x| x.rate() as usize)));
    let _ = l.register(err_cell(8, OP_LINE, case_code(8)));
    let _ = l.register(err_cell(9, OP_PROT, case_code(9)));
    let _ = l.register(err_cell(10, OP_PROT, case_code(10)));
    let _ = l.register(err_cell(11, OP_FLAGS, case_code(11)));
    let _ = l.register(err_cell(12, OP_RELOC, case_code(12)));
    let _ = l.register(err_cell(13, OP_FLAGS, case_code(13)));
    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 路线与保护位。
fn line_ok() -> bool {
    let a = Line::Glibc.label() == "glibc" && Line::Musl.label() == "musl";
    let b = Line::Glibc.allows_textrel() && !Line::Musl.allows_textrel();
    let c = Prot::RX.is_text() && !Prot::RX.writable() && Prot::RWX.writable();
    let d = Prot::RX.bits() == (PROT_READ | PROT_EXEC);
    let e = Prot::RW.bits() == (PROT_READ | PROT_WRITE);
    let f = Prot::RX.label() == "r-x" && Prot::RWX.label() == "rwx";
    let g = !Prot::RW.is_text() && Prot::R.label() == "r--";
    a && b && c && d && e && f && g
}

/// 重定位种类与文本偏移。
fn reloc_ok() -> bool {
    let a = RelocKind::Abs64.code() == 1 && RelocKind::Abs32.code() == 10;
    let b = RelocKind::Pc32.code() == 2 && !RelocKind::Unknown.known();
    let c = RelocKind::Abs64.label() == "abs64" && RelocKind::Unknown.label() == "unknown";
    let d = TextReloc::text(0x1000, RelocKind::Abs64, "foo").valid();
    let e = !TextReloc::text(0x1000, RelocKind::Unknown, "foo").valid();
    let f = TextReloc::data(0x2000, RelocKind::Abs64, "x").in_text == false;
    let g = TextReloc::weak_text(0x1010, RelocKind::Pc32, "w").weak;
    a && b && c && d && e && f && g
}

/// 旗标一致性。
fn flags_ok() -> bool {
    let missing = build(Scenario::FlagMissing);
    let a = missing.text_count() == 1 && !missing.textrel_flag();
    let b = load(&missing).code() == TX_EFLAGS;
    let redundant = build(Scenario::DataFlag);
    let c = redundant.text_count() == 0 && redundant.textrel_flag();
    let d = load(&redundant).passed() && load(&redundant).warning;
    let e = load(&build(Scenario::CleanGlibc)).code() == 0;
    a && b && c && d && e
}

/// 双线洁净放行。
fn clean_ok() -> bool {
    let g = load(&build(Scenario::CleanGlibc));
    let m = load(&build(Scenario::CleanMusl));
    let a = g.passed() && !g.warning && g.patched == 0 && g.mprotect_calls == 0;
    let b = m.passed() && m.patched == 0 && m.restored();
    let c = g.text == Prot::RX && m.text == Prot::RX;
    let d = load(&build(Scenario::MuslData)).passed();
    a && b && c && d
}

/// glibc 单条文本重定位。
fn single_ok() -> bool {
    let v = load(&build(Scenario::SingleText));
    let a = v.passed() && v.patched == 1 && v.mprotect_calls == 1;
    let b = v.restored() && v.text == Prot::RX && v.warning;
    let c = v.trace.count(Stage::Mprotect) == 1 && v.trace.count(Stage::Restore) == 1;
    let d = v.text().as_str() == "textrel-ok ./libtext.so line=glibc patched=1 prot=r-x warn=1";
    a && b && c && d
}

/// glibc 双条一次窗口。
fn pair_ok() -> bool {
    let v = load(&build(Scenario::PairText));
    v.passed()
        && v.patched == 2
        && v.mprotect_calls == 1
        && v.data_patched == 0
        && v.trace.count(Stage::Apply) == 1
        && v.restored()
}

/// 弱符号文本重定位。
fn weak_ok() -> bool {
    let v = load(&build(Scenario::WeakText));
    v.passed() && v.patched == 1 && v.weak_patched == 1 && v.mprotect_calls == 1
}

/// RELRO 交叉。
fn relro_ok() -> bool {
    let img = build(Scenario::RelroText);
    let v = load(&img);
    img.relro && v.passed() && v.patched == 1 && v.restored()
}

/// 冗余旗标数据槽与 musl 数据槽。
fn data_ok() -> bool {
    let d = load(&build(Scenario::DataFlag));
    let m = load(&build(Scenario::MuslData));
    let a = d.passed() && d.patched == 0 && d.data_patched == 1;
    let b = d.warning && d.mprotect_calls == 0 && d.restored();
    let c = m.passed() && m.data_patched == 1 && m.mprotect_calls == 0;
    a && b && c
}

/// 双线对照。
fn contrast_ok() -> bool {
    let g = load(&build(Scenario::SingleText));
    let m = load(&build(Scenario::MuslText));
    let a = g.passed() && !m.passed();
    let b = m.code() == TX_EMUSL && m.patched == 0;
    let c = m.text().as_str()
        == "textrel-reject ./libtext.so line=musl patched=0 prot=r-x code=0x0000fe52";
    let d = g.mprotect_calls == 1 && m.mprotect_calls == 0;
    a && b && c && d
}

/// 六类拒收。
fn rejects_ok() -> bool {
    let a = load(&build(Scenario::MuslText)).code() == TX_EMUSL;
    let b = load(&build(Scenario::MprotectFail)).code() == TX_EGLIB;
    let c = load(&build(Scenario::RestoreFail)).code() == TX_EMPROT;
    let d = load(&build(Scenario::FlagMissing)).code() == TX_EFLAGS;
    let e = load(&build(Scenario::RelocBad)).code() == TX_ERELOC;
    let f = load(&build(Scenario::LazyTextrel)).code() == TX_EBIND;
    a && b && c && d && e && f
}

/// 保护恢复面。
fn protect_ok() -> bool {
    let fail = load(&build(Scenario::RestoreFail));
    let stop = load(&build(Scenario::MprotectFail));
    let a = fail.code() == TX_EMPROT && !fail.restored();
    let b = fail.text == Prot::RWX && fail.mprotect_calls == 1;
    let c = stop.code() == TX_EGLIB && stop.text == Prot::RX && stop.mprotect_calls == 0;
    let d = stop.trace.last() == Some((Stage::Mprotect, false, 1));
    a && b && c && d
}

/// 轨迹。
fn trace_ok() -> bool {
    let good = load(&build(Scenario::PairText));
    let a = good.trace.at(0) == Some((Stage::Load, true, 2));
    let b = good.trace.last() == Some((Stage::Done, true, 0));
    let c = good.trace.count(Stage::Mprotect) == 1 && good.trace.count(Stage::Restore) == 1;
    let d = good.trace.count(Stage::Flags) == 1 && good.trace.count(Stage::Reloc) == 1;
    let bad = load(&build(Scenario::LazyTextrel));
    let e = bad.trace.last() == Some((Stage::Bind, false, 1));
    let f = Stage::Mprotect.label() == "mprotect" && Stage::Restore.label() == "restore";
    a && b && c && d && e && f
}

/// 语料形态。
fn corpus_ok() -> bool {
    if CASES.len() != CORPUS {
        return false;
    }
    let mut ok = 0usize;
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < CASES.len() {
        match CASES.get(i) {
            Some(c) => {
                if c.accept() {
                    ok += 1;
                } else {
                    bad += 1;
                }
            }
            None => return false,
        }
        i += 1;
    }
    ok == 8 && bad == 6
}

/// 槽。
fn slots_ok() -> bool {
    unique()
        && notes_ok()
        && table_ok()
        && cover(0).is_ok()
        && cover(14).is_ok()
        && cover(15) == Err(TX_ESLOT)
        && slot_at(14).is_none()
        && slot_at(0).map(|c| c.scenario) == Some(Scenario::CleanGlibc)
        && slot_at(13).map(|c| c.scenario) == Some(Scenario::LazyTextrel)
}

/// 渲染。
fn report_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.render()
        == "textrel corpus=14 accept=8 reject=6 glibc=1 musl=1 mprotect=1 flags=3 match=14 rate=1000ppm\n";
    let b = r.rate() == 1000 && r.matched() == 14;
    let c = r.green() && r.glibc() + r.musl() + r.mprotect() + r.flags() == 6;
    a && b && c
}

/// 红档。
fn reds_ok() -> bool {
    let red = match run_with(Some((5, TX_EMUSL))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = red.matched() == 13 && red.rate() == 929;
    let b = !red.green() && red.gate() == Err(TX_ESTATE);
    let c = run_with(Some((14, TX_EMUSL))) == Err(TX_ESLOT);
    let d = run_with(Some((0, 0))) == Err(TX_EVALUE);
    a && b && c && d
}

/// 指纹。
fn hash_ok() -> bool {
    let a = u32_text(0) == "0" && u32_text(929) == "929";
    let b = hex_text(0xfe50) == "0x0000fe50" && hex_text(3) == "0x00000003";
    let c = fold_word(FNV_OFF, 0) == digest(&[0u8; 8]);
    let d = digest(b"UNX") != digest(b"UNY");
    a && b && c && d
}

/// 冻结重放。
fn pin_ok() -> bool {
    let mut l = build_ledger();
    let a = l.audit() == Ok(()) && l.len() == SAMPLES;
    let b = l.ok_count() == 8 && l.err_count() == 6;
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let c = root != 0 && l.root() == root;
    let d = l.replay() == Ok(root);
    let e = l.register(Sample::blank()) == Err(TX_ESEAL);
    let f = l.rejects() == 1 && l.freeze() == Err(TX_ESTATE);
    let g = l.sample(7).map(|s| s.value) == Some(929);
    let h = build_ledger().seal() == root;
    a && b && c && d && e && f && g && h
}

/// 四注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = line_ok() && reloc_ok() && flags_ok();
    out[1] = clean_ok() && single_ok() && pair_ok() && weak_ok();
    out[2] = relro_ok() && data_ok() && contrast_ok() && protect_ok();
    out[3] = rejects_ok()
        && trace_ok()
        && corpus_ok()
        && slots_ok()
        && report_ok()
        && reds_ok()
        && hash_ok()
        && pin_ok()
        && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11632_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11632.textrel_policy");

    s.add("TX01-装载路线与保护位", line_ok(), "static");
    s.add("TX02-重定位种类与文本偏移", reloc_ok(), "static");
    s.add("TX03-TEXTREL 旗标一致性", flags_ok(), "static");
    s.add("TX04-双线洁净放行", clean_ok(), "static");
    s.add("TX05-glibc 单条文本重定位", single_ok(), "static");
    s.add("TX06-glibc 双条一次窗口", pair_ok(), "static");
    s.add("TX07-弱符号文本重定位", weak_ok(), "static");
    s.add("TX08-RELRO 交叉与冗余旗标", relro_ok() && data_ok(), "static");
    s.add("TX09-musl 明拒与 glibc 允许对照", contrast_ok(), "static");
    s.add("TX10-mprotect 与恢复失败面", protect_ok(), "static");
    s.add("TX11-六类策略拒收", rejects_ok(), "static");
    s.add(
        "TX12-十四槽渲染与红档",
        trace_ok() && corpus_ok() && slots_ok() && report_ok() && reds_ok(),
        "static",
    );
    s.add("TX13-冻结重放四注入", inject_four() == [true; 4], "static");

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_helpers() {
        assert_eq!(u32_text(0).as_str(), "0");
        assert_eq!(u32_text(14).as_str(), "14");
        assert_eq!(u32_text(929).as_str(), "929");
        assert_eq!(hex_text(0xfe50).as_str(), "0x0000fe50");
        assert_eq!(hex_text(0xfe5d).as_str(), "0x0000fe5d");
    }

    #[test]
    fn fnv_known() {
        let mut h = FNV_OFF;
        h = fold_byte(h, b'a');
        assert_eq!(h, 0xaf63dc4c8601ec8c);
        assert_ne!(fold_word(FNV_OFF, 2), FNV_OFF);
        assert_eq!(fold_word(FNV_OFF, 0), digest(&[0u8; 8]));
    }

    #[test]
    fn line_semantics() {
        assert!(Line::Glibc.allows_textrel());
        assert!(!Line::Musl.allows_textrel());
        assert_eq!(Line::Glibc.label(), "glibc");
    }

    #[test]
    fn prot_bits() {
        assert_eq!(Prot::RX.bits(), 5);
        assert_eq!(Prot::RW.bits(), 3);
        assert_eq!(Prot::RWX.bits(), 7);
        assert!(Prot::RX.is_text());
        assert!(!Prot::RX.writable());
        assert_eq!(Prot::RWX.label(), "rwx");
    }

    #[test]
    fn reloc_codes() {
        assert_eq!(RelocKind::Abs64.code(), 1);
        assert_eq!(RelocKind::Abs32.code(), 10);
        assert_eq!(RelocKind::Pc32.code(), 2);
        assert_eq!(RelocKind::Unknown.code(), 99);
        assert!(!RelocKind::Unknown.known());
        assert_eq!(RelocKind::Abs64.label(), "abs64");
    }

    #[test]
    fn reloc_validity() {
        assert!(TextReloc::text(0x1000, RelocKind::Abs64, "foo").valid());
        assert!(!TextReloc::text(0x1000, RelocKind::Unknown, "foo").valid());
        assert!(!TextReloc::data(0x2000, RelocKind::Abs64, "x").in_text);
        assert!(TextReloc::weak_text(0x1010, RelocKind::Pc32, "w").weak);
    }

    #[test]
    fn image_counts() {
        let img = build(Scenario::PairText);
        assert_eq!(img.len(), 2);
        assert_eq!(img.text_count(), 2);
        assert_eq!(img.data_count(), 0);
        let w = build(Scenario::WeakText);
        assert_eq!(w.weak_count(), 1);
        let d = build(Scenario::DataFlag);
        assert_eq!(d.text_count(), 0);
        assert_eq!(d.data_count(), 1);
    }

    #[test]
    fn clean_both_lines() {
        let g = load(&build(Scenario::CleanGlibc));
        assert!(g.passed());
        assert!(g.restored());
        assert_eq!(g.patched, 0);
        let m = load(&build(Scenario::CleanMusl));
        assert!(m.passed());
        assert_eq!(m.line, Line::Musl);
    }

    #[test]
    fn musl_data_only() {
        let v = load(&build(Scenario::MuslData));
        assert!(v.passed());
        assert_eq!(v.data_patched, 1);
        assert_eq!(v.mprotect_calls, 0);
        assert!(v.restored());
    }

    #[test]
    fn glibc_single() {
        let v = load(&build(Scenario::SingleText));
        assert!(v.passed());
        assert_eq!(v.patched, 1);
        assert_eq!(v.mprotect_calls, 1);
        assert!(v.warning);
        assert_eq!(v.trace.count(Stage::Mprotect), 1);
        assert_eq!(v.trace.count(Stage::Restore), 1);
    }

    #[test]
    fn glibc_pair_window() {
        let v = load(&build(Scenario::PairText));
        assert!(v.passed());
        assert_eq!(v.patched, 2);
        assert_eq!(v.mprotect_calls, 1);
        assert_eq!(v.trace.count(Stage::Apply), 1);
    }

    #[test]
    fn weak_text_patched() {
        let v = load(&build(Scenario::WeakText));
        assert!(v.passed());
        assert_eq!(v.patched, 1);
        assert_eq!(v.weak_patched, 1);
    }

    #[test]
    fn relro_with_textrel() {
        let img = build(Scenario::RelroText);
        assert!(img.relro);
        let v = load(&img);
        assert!(v.passed());
        assert_eq!(v.patched, 1);
        assert!(v.restored());
    }

    #[test]
    fn redundant_flag_tolerated() {
        let v = load(&build(Scenario::DataFlag));
        assert!(v.passed());
        assert_eq!(v.patched, 0);
        assert_eq!(v.data_patched, 1);
        assert!(v.warning);
        assert_eq!(v.mprotect_calls, 0);
    }

    #[test]
    fn musl_rejects_textrel() {
        let img = build(Scenario::MuslText);
        assert_eq!(img.text_count(), 1);
        let v = load(&img);
        assert_eq!(v.code(), TX_EMUSL);
        assert_eq!(v.patched, 0);
        assert_eq!(v.mprotect_calls, 0);
    }

    #[test]
    fn glibc_allows_same_image() {
        let g = load(&build(Scenario::SingleText));
        assert!(g.passed());
        assert_eq!(g.mprotect_calls, 1);
        assert!(g.restored());
    }

    #[test]
    fn mprotect_failure() {
        let v = load(&build(Scenario::MprotectFail));
        assert_eq!(v.code(), TX_EGLIB);
        assert_eq!(v.text, Prot::RX);
        assert_eq!(v.trace.last(), Some((Stage::Mprotect, false, 1)));
    }

    #[test]
    fn restore_failure_leaves_rwx() {
        let v = load(&build(Scenario::RestoreFail));
        assert_eq!(v.code(), TX_EMPROT);
        assert!(!v.restored());
        assert_eq!(v.text, Prot::RWX);
        assert_eq!(v.mprotect_calls, 1);
    }

    #[test]
    fn flag_missing_rejected() {
        let img = build(Scenario::FlagMissing);
        assert!(img.text_count() > 0);
        assert!(!img.textrel_flag());
        let v = load(&img);
        assert_eq!(v.code(), TX_EFLAGS);
        assert_eq!(v.trace.last(), Some((Stage::Flags, false, 1)));
    }

    #[test]
    fn bad_reloc_rejected() {
        let img = build(Scenario::RelocBad);
        assert!(img.bad_reloc());
        let v = load(&img);
        assert_eq!(v.code(), TX_ERELOC);
    }

    #[test]
    fn lazy_textrel_rejected() {
        let img = build(Scenario::LazyTextrel);
        assert!(img.lazy_bind && img.text_count() == 1);
        let v = load(&img);
        assert_eq!(v.code(), TX_EBIND);
        assert_eq!(v.trace.last(), Some((Stage::Bind, false, 1)));
    }

    #[test]
    fn verdict_text() {
        let v = load(&build(Scenario::SingleText));
        assert_eq!(
            v.text().as_str(),
            "textrel-ok ./libtext.so line=glibc patched=1 prot=r-x warn=1"
        );
        let m = load(&build(Scenario::MuslText));
        assert_eq!(
            m.text().as_str(),
            "textrel-reject ./libtext.so line=musl patched=0 prot=r-x code=0x0000fe52"
        );
    }

    #[test]
    fn trace_shape() {
        let v = load(&build(Scenario::PairText));
        assert_eq!(v.trace.len(), 8);
        assert_eq!(v.trace.at(0), Some((Stage::Load, true, 2)));
        assert_eq!(v.trace.last(), Some((Stage::Done, true, 0)));
        assert_eq!(v.trace.count(Stage::Flags), 1);
        assert_eq!(v.trace.count(Stage::Reloc), 1);
        assert_eq!(v.trace.count(Stage::Bind), 1);
        assert_eq!(v.trace.count(Stage::Mprotect), 1);
        assert_eq!(v.trace.count(Stage::Apply), 1);
        assert_eq!(v.trace.count(Stage::Restore), 1);
    }

    #[test]
    fn corpus_shape() {
        assert!(unique());
        assert!(notes_ok());
        assert!(table_ok());
        assert_eq!(CASES.len(), 14);
        assert!(slot_at(14).is_none());
    }

    #[test]
    fn corpus_counts() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => Report::new(),
        };
        assert_eq!(r.total(), 14);
        assert_eq!(r.accept(), 8);
        assert_eq!(r.reject(), 6);
        assert_eq!(r.glibc(), 1);
        assert_eq!(r.musl(), 1);
        assert_eq!(r.mprotect(), 1);
        assert_eq!(r.flags(), 3);
        assert!(r.green());
    }

    #[test]
    fn render_literal() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => Report::new(),
        };
        assert_eq!(
            r.render().as_str(),
            "textrel corpus=14 accept=8 reject=6 glibc=1 musl=1 mprotect=1 flags=3 match=14 rate=1000ppm\n"
        );
    }

    #[test]
    fn red_rate() {
        let r = match run_with(Some((2, TX_EMUSL))) {
            Ok(v) => v,
            Err(_) => Report::new(),
        };
        assert_eq!(r.rate(), 929);
        assert_eq!(r.matched(), 13);
        assert_eq!(r.gate(), Err(TX_ESTATE));
    }

    #[test]
    fn cover_prefix() {
        assert_eq!(cover(0), Ok(()));
        assert_eq!(cover(14), Ok(()));
        assert_eq!(cover(15), Err(TX_ESLOT));
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert!(!l.is_empty());
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert!(l.sample(0).is_some());
        assert!(l.sample(SAMPLES).is_none());
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        let root = match l.freeze() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_ne!(root, 0);
        assert_eq!(l.root(), root);
        assert!(l.is_frozen());
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.register(Sample::blank()), Err(TX_ESEAL));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn injections() {
        assert_eq!(inject_four(), [true; 4]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11632_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }

    #[test]
    fn error_codes_distinct() {
        let codes = [
            TX_EGLIB,
            TX_EMUSL,
            TX_EMPROT,
            TX_EFLAGS,
            TX_ERELOC,
            TX_EBIND,
            TX_ESTATE,
            TX_EVERIFY,
            TX_EBAL,
            TX_ESLOT,
            TX_EVALUE,
            TX_ESEAL,
            TX_ENOBS,
        ];
        let mut i = 0usize;
        while i < codes.len() {
            assert_eq!(codes[i], SEG_BASE + 1 + i as u32);
            i += 1;
        }
    }
}
