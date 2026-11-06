//! UNX-F11604 程序头 p_offset/p_filesz 越界 mmap 拒收
//!
//! load_elf_binary 对每个 PT_LOAD 做一次 mmap：文件区间 `[p_offset,
//! p_offset+p_filesz)` 必须落在文件里，虚拟区间 `[p_vaddr, p_vaddr+p_memsz)`
//! 必须能整段覆盖文件区间（`p_filesz <= p_memsz`），两个区间的尾地址都不许
//! 回绕。任一条不成立，这段镜像就是坏的，继续 mmap 等于把文件外的页映射进
//! 用户地址空间。
//!
//! 三种正常形态要分开记：有文件内容的段（map）、只有 bss 的段（filesz=0，
//! 只占内存不占文件）、以及带 bss 尾巴的段（filesz<memsz，映射后补零）。跳
//! 过任何一个都会让统计把「没内容的段」误算成「坏段」。
//!
//! 判据次序固定：先认段类型（非 PT_LOAD 不许走映射路径），再看是否空段，
//! 再看 filesz 与 memsz 的大小关系，再算文件区间尾地址（乘法不走，加法全
//! checked），最后算虚拟区间尾地址。溢出档单独立码，避免回绕后的小地址被
//! 当成合法尾地址。
//!
//! 闸门次序：字节 → 名称 → 段算术 → 页换算 → 语料 → 文案 → 错误码 → 绿档。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本 14 格
//! （8 成 6 败）在 `audit` 逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfb30;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 页大小。
pub const PAGE: u64 = 0x1000;

/// PT_LOAD。
pub const PT_LOAD: u32 = 1;

/// PT_DYNAMIC。
pub const PT_DYNAMIC: u32 = 2;

/// PT_NOTE。
pub const PT_NOTE: u32 = 4;

/// 名称容量。
pub const NAME_CAP: usize = 16;

/// 语料条数。
pub const CORPUS: usize = 14;

/// 映射模式：只读。
pub const PROT_READ: u8 = 0x1;

/// 映射模式：读写。
pub const PROT_RW: u8 = 0x3;

/// 映射模式：读写执行。
pub const PROT_RWX: u8 = 0x7;

/// `ENOEXEC`。
pub const ENOEXEC: i32 = 8;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 段偏移越界。
pub const LD_EOFF: u32 = SEG_BASE + 0x01;
/// filesz 大于 memsz。
pub const LD_ESIZE: u32 = SEG_BASE + 0x02;
/// 地址回绕。
pub const LD_EWRAP: u32 = SEG_BASE + 0x03;
/// 非 LOAD 段。
pub const LD_ETYPE: u32 = SEG_BASE + 0x04;
/// 状态冲突。
pub const LD_ESTATE: u32 = SEG_BASE + 0x05;
/// 未达绿档。
pub const LD_EVER: u32 = SEG_BASE + 0x06;
/// 字节非法。
pub const LD_EBYTE: u32 = SEG_BASE + 0x07;
/// 段号非法。
pub const LD_ESEG: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const LD_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const LD_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const LD_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const LD_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const LD_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 段装载。
pub const OP_SEG: u8 = 0x01;
/// 文件区间。
pub const OP_FILESZ: u8 = 0x02;
/// 虚拟区间。
pub const OP_VADDR: u8 = 0x03;
/// 页换算。
pub const OP_PAGE: u8 = 0x04;
/// 文案。
pub const OP_MSG: u8 = 0x05;
/// 语料全跑。
pub const OP_CORPUS: u8 = 0x06;

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
// 渲染
// ---------------------------------------------------------------------------

/// 追加一个无符号十进制数。
fn push_dec(out: &mut String, v: u32) {
    if v == 0 {
        out.push('0');
        return;
    }
    let mut n = v;
    let mut buf = [0u8; 10];
    let mut len = 0usize;
    while n > 0 {
        let d = (n % 10) as u8;
        if let Some(dst) = buf.get_mut(len) {
            *dst = b'0' + d;
        }
        n /= 10;
        len += 1;
    }
    while len > 0 {
        len -= 1;
        if let Some(b) = buf.get(len) {
            out.push(*b as char);
        }
    }
}

/// 追加一个 64 位数。
fn push_dec64(out: &mut String, v: u64) {
    if v == 0 {
        out.push('0');
        return;
    }
    let mut n = v;
    let mut buf = [0u8; 20];
    let mut len = 0usize;
    while n > 0 {
        let d = (n % 10) as u8;
        if let Some(dst) = buf.get_mut(len) {
            *dst = b'0' + d;
        }
        n /= 10;
        len += 1;
    }
    while len > 0 {
        len -= 1;
        if let Some(b) = buf.get(len) {
            out.push(*b as char);
        }
    }
}

// ---------------------------------------------------------------------------
// 名称
// ---------------------------------------------------------------------------

/// 定长名称。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Name {
    buf: [u8; NAME_CAP],
    len: usize,
}

impl Name {
    /// 装载：可打印 ASCII，16 字节内。
    pub fn new(bytes: &[u8]) -> Result<Name, u32> {
        if bytes.is_empty() {
            return Err(LD_EBYTE);
        }
        if bytes.len() > NAME_CAP {
            return Err(LD_EBYTE);
        }
        let mut buf = [0u8; NAME_CAP];
        let mut i = 0usize;
        while i < bytes.len() {
            let b = match bytes.get(i) {
                Some(v) => *v,
                None => return Err(LD_ENOBS),
            };
            if b < 0x20 || b == 0x7f || b >= 0x80 {
                return Err(LD_EBYTE);
            }
            if let Some(dst) = buf.get_mut(i) {
                *dst = b;
            }
            i += 1;
        }
        Ok(Name { buf, len: bytes.len() })
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 字节视图。
    pub fn bytes(&self) -> &[u8] {
        self.buf.get(..self.len).unwrap_or(&[])
    }

    /// 逐字节比对。
    pub fn same(&self, other: &[u8]) -> bool {
        if self.len != other.len() {
            return false;
        }
        let mut i = 0usize;
        while i < self.len {
            match (self.buf.get(i), other.get(i)) {
                (Some(a), Some(b)) => {
                    if a != b {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 文本视图。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(b) = self.buf.get(i) {
                s.push(*b as char);
            }
            i += 1;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 段
// ---------------------------------------------------------------------------

/// 一个程序头段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    seg_type: u32,
    flags: u8,
    offset: u64,
    filesz: u64,
    memsz: u64,
    vaddr: u64,
}

impl Segment {
    /// 构造。
    pub const fn new(t: u32, flags: u8, offset: u64, filesz: u64, memsz: u64, vaddr: u64) -> Segment {
        Segment { seg_type: t, flags, offset, filesz, memsz, vaddr }
    }

    /// 段类型。
    pub fn seg_type(&self) -> u32 {
        self.seg_type
    }

    /// 权限位。
    pub fn flags(&self) -> u8 {
        self.flags
    }

    /// 文件偏移。
    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// 文件长度。
    pub fn filesz(&self) -> u64 {
        self.filesz
    }

    /// 内存长度。
    pub fn memsz(&self) -> u64 {
        self.memsz
    }

    /// 虚拟地址。
    pub fn vaddr(&self) -> u64 {
        self.vaddr
    }

    /// 是否 LOAD 段。
    pub fn is_load(&self) -> bool {
        self.seg_type == PT_LOAD
    }

    /// 是否只有 bss。
    pub fn bss_only(&self) -> bool {
        self.filesz == 0
    }

    /// 文件区间尾地址。
    pub fn file_end(&self) -> Option<u64> {
        self.offset.checked_add(self.filesz)
    }

    /// 虚拟区间尾地址。
    pub fn virt_end(&self) -> Option<u64> {
        self.vaddr.checked_add(self.memsz)
    }

    /// bss 尾巴长度。
    pub fn bss_len(&self) -> u64 {
        self.memsz.saturating_sub(self.filesz)
    }

    /// 映射权限。
    pub fn prot(&self) -> u8 {
        self.flags
    }

    /// 映射页范围（检查通过后才有意义）。
    pub fn page_span(&self) -> Result<(u64, u64), u32> {
        let start = page_down(self.vaddr);
        let last = match self.virt_end() {
            Some(v) => v,
            None => return Err(LD_EWRAP),
        };
        Ok((start, page_up(last)?))
    }

    /// 段闸门。
    pub fn check(&self, file_len: u64) -> Result<(), u32> {
        if !self.is_load() {
            return Err(LD_ETYPE);
        }
        if self.filesz > self.memsz {
            return Err(LD_ESIZE);
        }
        match self.file_end() {
            Some(end) => {
                if end > file_len {
                    return Err(LD_EOFF);
                }
            }
            None => return Err(LD_EWRAP),
        }
        match self.virt_end() {
            Some(_) => {}
            None => return Err(LD_EWRAP),
        }
        Ok(())
    }
}

/// 页对齐向下。
fn page_down(v: u64) -> u64 {
    v & !(PAGE - 1)
}

/// 页对齐向上（checked）。
fn page_up(v: u64) -> Result<u64, u32> {
    if v == 0 {
        return Ok(0);
    }
    let x = v.checked_add(PAGE - 1).ok_or(LD_EWRAP)?;
    Ok(page_down(x))
}

/// 判定结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Judgement {
    /// 0 映射 / 1 空段 / 2 拒收。
    state: u8,
    /// 错误码；通过为 0。
    code: u32,
    /// 文件区间尾地址（溢出为 0）。
    end: u64,
    /// bss 字节数。
    bss: u64,
}

impl Judgement {
    /// 判定一段。
    pub fn of(s: &Segment, file_len: u64) -> Judgement {
        let r = s.check(file_len);
        let state = if r.is_ok() {
            if s.bss_only() {
                1
            } else {
                0
            }
        } else {
            2
        };
        Judgement {
            state,
            code: match r {
                Ok(()) => 0,
                Err(c) => c,
            },
            end: s.file_end().unwrap_or(0),
            bss: s.bss_len(),
        }
    }

    /// 状态码。
    pub fn state(&self) -> u8 {
        self.state
    }

    /// 错误码。
    pub fn code(&self) -> u32 {
        self.code
    }

    /// 尾地址。
    pub fn end(&self) -> u64 {
        self.end
    }

    /// bss 长度。
    pub fn bss(&self) -> u64 {
        self.bss
    }

    /// 是否映射。
    pub fn mapped(&self) -> bool {
        self.state == 0
    }

    /// 是否空段。
    pub fn empty(&self) -> bool {
        self.state == 1
    }

    /// 是否拒收。
    pub fn rejected(&self) -> bool {
        self.state == 2
    }
}

/// 文案。
pub fn message(s: &Segment, file_len: u64) -> String {
    let j = Judgement::of(s, file_len);
    let mut out = String::new();
    if j.mapped() {
        out.push_str("ok: PT_LOAD map off=");
        push_dec64(&mut out, s.offset());
        out.push_str(" filesz=");
        push_dec64(&mut out, s.filesz());
        out.push_str(" bss=");
        push_dec64(&mut out, j.bss());
        out.push_str(" prot=");
        push_dec(&mut out, s.prot() as u32);
        return out;
    }
    if j.empty() {
        out.push_str("ok: PT_LOAD bss-only memsz=");
        push_dec64(&mut out, s.memsz());
        return out;
    }
    out.push_str("bash: ./a.out: cannot execute binary file: Exec format error (pt_load ");
    match j.code() {
        LD_EOFF => {
            out.push_str("off=");
            push_dec64(&mut out, s.offset());
            out.push_str(" filesz=");
            push_dec64(&mut out, s.filesz());
            out.push_str(" past file=");
            push_dec64(&mut out, file_len);
        }
        LD_ESIZE => {
            out.push_str("filesz=");
            push_dec64(&mut out, s.filesz());
            out.push_str(" > memsz=");
            push_dec64(&mut out, s.memsz());
        }
        LD_EWRAP => {
            out.push_str("address wrap");
        }
        LD_ETYPE => {
            out.push_str("type=");
            push_dec(&mut out, s.seg_type());
            out.push_str(" not PT_LOAD");
        }
        _ => out.push_str("rejected"),
    }
    out.push(')');
    out
}

/// 错误码：段问题一律 ENOEXEC。
pub fn errno(s: &Segment, file_len: u64) -> i32 {
    match Judgement::of(s, file_len).state {
        0 | 1 => 0,
        _ => ENOEXEC,
    }
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 一条语料。
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    /// 编号（1 起）。
    pub no: u8,
    /// 标签。
    pub tag: &'static [u8],
    /// 段。
    pub seg: Segment,
    /// 文件长度。
    pub file: u64,
    /// 期望状态（0 映射 / 1 空 / 2 拒收）。
    pub want_state: u8,
    /// 期望错误码；0 表示通过。
    pub want_err: u32,
}

impl Slot {
    /// 标签名。
    pub fn name(&self) -> Result<Name, u32> {
        Name::new(self.tag)
    }

    /// 是否命中。
    pub fn hit(&self) -> bool {
        let j = Judgement::of(&self.seg, self.file);
        j.state() == self.want_state && j.code() == self.want_err
    }
}

/// 十四卡段语料。
pub static CORPUS14: [Slot; CORPUS] = [
    Slot {
        no: 1,
        tag: b"text_rx",
        seg: Segment::new(PT_LOAD, PROT_RWX, 0, 0x1000, 0x2000, 0x400000),
        file: 10000,
        want_state: 0,
        want_err: 0,
    },
    Slot {
        no: 2,
        tag: b"rodata_r",
        seg: Segment::new(PT_LOAD, PROT_READ, 0x1000, 0x200, 0x200, 0x401000),
        file: 10000,
        want_state: 0,
        want_err: 0,
    },
    Slot {
        no: 3,
        tag: b"data_rw",
        seg: Segment::new(PT_LOAD, PROT_RW, 0x2000, 0x80, 0x80, 0x402000),
        file: 10000,
        want_state: 0,
        want_err: 0,
    },
    Slot {
        no: 4,
        tag: b"bss_only",
        seg: Segment::new(PT_LOAD, PROT_RW, 0, 0, 0x1000, 0x600000),
        file: 10000,
        want_state: 1,
        want_err: 0,
    },
    Slot {
        no: 5,
        tag: b"both_zero",
        seg: Segment::new(PT_LOAD, PROT_RW, 0x2000, 0, 0, 0x600000),
        file: 10000,
        want_state: 1,
        want_err: 0,
    },
    Slot {
        no: 6,
        tag: b"filesz_big",
        seg: Segment::new(PT_LOAD, PROT_RW, 0x1000, 0x2000, 0x1000, 0x400000),
        file: 10000,
        want_state: 2,
        want_err: 0xfb32,
    },
    Slot {
        no: 7,
        tag: b"past_file",
        seg: Segment::new(PT_LOAD, PROT_READ, 0x9000, 0x2000, 0x2000, 0x400000),
        file: 10000,
        want_state: 2,
        want_err: 0xfb31,
    },
    Slot {
        no: 8,
        tag: b"wrap_off",
        seg: Segment::new(PT_LOAD, PROT_READ, u64::MAX - 15, 0x1000, 0x1000, 0x400000),
        file: u64::MAX,
        want_state: 2,
        want_err: 0xfb33,
    },
    Slot {
        no: 9,
        tag: b"wrap_vaddr",
        seg: Segment::new(PT_LOAD, PROT_READ, 0x1000, 0x10, 0x1000, u64::MAX - 15),
        file: 10000,
        want_state: 2,
        want_err: 0xfb33,
    },
    Slot {
        no: 10,
        tag: b"dynamic",
        seg: Segment::new(PT_DYNAMIC, PROT_RW, 0x1000, 0x10, 0x10, 0x400000),
        file: 10000,
        want_state: 2,
        want_err: 0xfb34,
    },
    Slot {
        no: 11,
        tag: b"note",
        seg: Segment::new(PT_NOTE, PROT_READ, 0x1000, 0x10, 0x10, 0x400000),
        file: 10000,
        want_state: 2,
        want_err: 0xfb34,
    },
    Slot {
        no: 12,
        tag: b"end_exact",
        seg: Segment::new(PT_LOAD, PROT_READ, 0x1000, 0x1000, 0x1000, 0x400000),
        file: 0x2000,
        want_state: 0,
        want_err: 0,
    },
    Slot {
        no: 13,
        tag: b"off_at_end",
        seg: Segment::new(PT_LOAD, PROT_READ, 0x2000, 0x10, 0x10, 0x400000),
        file: 0x2000,
        want_state: 2,
        want_err: 0xfb31,
    },
    Slot {
        no: 14,
        tag: b"bss_tail",
        seg: Segment::new(PT_LOAD, PROT_RW, 0x1000, 0x100, 0x10000, 0x400000),
        file: 10000,
        want_state: 0,
        want_err: 0,
    },
];

/// 按编号取语料。
pub fn corpus_no(no: u8) -> Result<&'static Slot, u32> {
    if no == 0 {
        return Err(LD_ENOBS);
    }
    match CORPUS14.get((no as usize) - 1) {
        Some(v) => Ok(v),
        None => Err(LD_ENOBS),
    }
}

// ---------------------------------------------------------------------------
// 汇总
// ---------------------------------------------------------------------------

/// 语料汇总。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    total: usize,
    map: usize,
    empty: usize,
    reject: usize,
    matched: usize,
}

impl Report {
    /// 空汇总。
    pub const fn new() -> Report {
        Report { total: 0, map: 0, empty: 0, reject: 0, matched: 0 }
    }

    /// 总数。
    pub fn total(&self) -> usize {
        self.total
    }

    /// 映射数。
    pub fn map(&self) -> usize {
        self.map
    }

    /// 空段数。
    pub fn empty(&self) -> usize {
        self.empty
    }

    /// 拒收数。
    pub fn reject(&self) -> usize {
        self.reject
    }

    /// 命中数。
    pub fn matched(&self) -> usize {
        self.matched
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// 命中率，千分比向上取整。
    pub fn rate(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        let n = (self.matched as u32).saturating_mul(1000);
        let t = self.total as u32;
        (n + t - 1) / t
    }

    /// 绿档闸门。
    pub fn gate(&self) -> Result<(), u32> {
        if self.total != CORPUS {
            return Err(LD_ENOBS);
        }
        if self.map != 5 || self.empty != 2 || self.reject != 7 {
            return Err(LD_EVER);
        }
        if self.matched != self.total {
            return Err(LD_EVER);
        }
        Ok(())
    }

    /// 是否绿。
    pub fn green(&self) -> bool {
        self.gate() == Ok(())
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("ptload corpus=");
        push_dec(&mut s, self.total as u32);
        s.push_str(" map=");
        push_dec(&mut s, self.map as u32);
        s.push_str(" empty=");
        push_dec(&mut s, self.empty as u32);
        s.push_str(" reject=");
        push_dec(&mut s, self.reject as u32);
        s.push_str(" match=");
        push_dec(&mut s, self.matched as u32);
        s.push_str(" rate=");
        push_dec(&mut s, self.rate());
        s.push_str("ppm\n");
        s
    }
}

/// 跑全部语料；`over` 改写某条期望错误码。
fn run_with(over: Option<(usize, u32)>) -> Result<Report, u32> {
    let mut r = Report::new();
    let mut i = 0usize;
    while i < CORPUS {
        let slot = match CORPUS14.get(i) {
            Some(v) => v,
            None => return Err(LD_ENOBS),
        };
        if slot.no as usize != i + 1 {
            return Err(LD_ESTATE);
        }
        let j = Judgement::of(&slot.seg, slot.file);
        r.total += 1;
        match j.state() {
            0 => r.map += 1,
            1 => r.empty += 1,
            _ => r.reject += 1,
        }
        let want_err = match over {
            Some((idx, c)) if idx == i => c,
            _ => slot.want_err,
        };
        if j.state() == slot.want_state && j.code() == want_err {
            r.matched += 1;
        }
        i += 1;
    }
    Ok(r)
}

/// 跑全量。
pub fn run() -> Result<Report, u32> {
    run_with(None)
}

/// 覆盖闸门。
pub fn cover(n: usize) -> Result<(), u32> {
    if n == CORPUS {
        Ok(())
    } else {
        Err(LD_ENOBS)
    }
}

// ---------------------------------------------------------------------------
// 映射器
// ---------------------------------------------------------------------------

/// 一次性映射器：同一段只许映射一次。
#[derive(Clone, Copy, Debug)]
pub struct Mapper {
    bound: bool,
    start: u64,
    end: u64,
}

impl Mapper {
    /// 空映射器。
    pub const fn new() -> Mapper {
        Mapper { bound: false, start: 0, end: 0 }
    }

    /// 映射一次；重复映射落 ESTATE。
    pub fn map(&mut self, s: &Segment, file_len: u64) -> Result<(), u32> {
        if self.bound {
            return Err(LD_ESTATE);
        }
        s.check(file_len)?;
        let (start, end) = s.page_span()?;
        if end < start {
            return Err(LD_EWRAP);
        }
        self.bound = true;
        self.start = start;
        self.end = end;
        Ok(())
    }

    /// 是否已绑定。
    pub fn bound(&self) -> bool {
        self.bound
    }

    /// 起始页。
    pub fn start(&self) -> u64 {
        self.start
    }

    /// 结束页。
    pub fn end(&self) -> u64 {
        self.end
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
    pub const fn new() -> Ledger {
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(LD_ESLOT);
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
            return Err(LD_EBAL);
        }
        const VALS: [u32; 8] = [14, 5, 2, 7, 4096, 8192, 16, 929];
        const CODES: [u32; 6] = [LD_ESIZE, LD_EOFF, LD_EWRAP, LD_ETYPE, LD_ESTATE, LD_EBYTE];
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
            return Err(LD_EBAL);
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
            Err(LD_ESEAL)
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

/// 建账本：8 成 6 败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();

    let _ = l.register(count_cell(0, OP_CORPUS, r.map(|x| x.total())));
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.map())));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.empty())));
    let _ = l.register(count_cell(3, OP_CORPUS, r.map(|x| x.reject())));

    let _ = l.register(count_cell(4, OP_PAGE, Ok(PAGE as usize)));
    let _ = l.register(count_cell(5, OP_FILESZ, Ok(0x2000usize)));
    let _ = l.register(count_cell(6, OP_VADDR, Ok(16usize)));

    let red = run_with(Some((0, LD_ETYPE)));
    let _ = l.register(count_cell(7, OP_SEG, red.map(|x| x.rate() as usize)));

    let size_err = Segment::new(PT_LOAD, PROT_RW, 0x1000, 0x2000, 0x1000, 0x400000)
        .check(10000)
        .err()
        .unwrap_or(0);
    let _ = l.register(err_cell(8, OP_SEG, if size_err == 0 { LD_ENOBS } else { size_err }));

    let off_err = Segment::new(PT_LOAD, PROT_READ, 0x9000, 0x2000, 0x2000, 0x400000)
        .check(10000)
        .err()
        .unwrap_or(0);
    let _ = l.register(err_cell(9, OP_FILESZ, if off_err == 0 { LD_ENOBS } else { off_err }));

    let wrap_err = Segment::new(PT_LOAD, PROT_READ, u64::MAX - 15, 0x1000, 0x1000, 0x400000)
        .check(u64::MAX)
        .err()
        .unwrap_or(0);
    let _ = l.register(err_cell(10, OP_VADDR, if wrap_err == 0 { LD_ENOBS } else { wrap_err }));

    let type_err = Segment::new(PT_DYNAMIC, PROT_RW, 0x1000, 0x10, 0x10, 0x400000)
        .check(10000)
        .err()
        .unwrap_or(0);
    let _ = l.register(err_cell(11, OP_SEG, if type_err == 0 { LD_ENOBS } else { type_err }));

    let mut mp = Mapper::new();
    let text = Segment::new(PT_LOAD, PROT_RWX, 0, 0x1000, 0x2000, 0x400000);
    let _ = mp.map(&text, 10000);
    let state_err = match mp.map(&text, 10000) {
        Ok(()) => LD_ESTATE,
        Err(c) => c,
    };
    let _ = l.register(err_cell(12, OP_SEG, state_err));

    let byte_err = match Name::new(&[0x00]) {
        Ok(_) => LD_EBYTE,
        Err(c) => c,
    };
    let _ = l.register(err_cell(13, OP_SEG, byte_err));

    l
}

// ---------------------------------------------------------------------------
// 检查面
// ---------------------------------------------------------------------------

/// 名称闸门。
fn name_ok() -> bool {
    let n = match Name::new(b"text_rx") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = n.len() == 7 && n.text() == String::from("text_rx") && n.same(b"text_rx");
    let b = Name::new(b"").err() == Some(LD_EBYTE);
    let c = Name::new(&[0u8; NAME_CAP + 1]).err() == Some(LD_EBYTE);
    let d = Name::new(&[0x00]).err() == Some(LD_EBYTE);
    a && b && c && d
}

/// 段算术。
fn arith_ok() -> bool {
    let s = Segment::new(PT_LOAD, PROT_RWX, 0x1000, 0x1000, 0x2000, 0x400000);
    let a = s.file_end() == Some(0x2000) && s.virt_end() == Some(0x402000);
    let b = s.bss_len() == 0x1000 && s.bss_only() == false && s.is_load();
    let c = s.prot() == PROT_RWX && s.flags() == 0x7;
    let e = Segment::new(PT_LOAD, 0, 0, 0, 0x1000, 0x600000).bss_only();
    let w = Segment::new(PT_LOAD, 0, u64::MAX - 15, 0x1000, 0x1000, 0x400000);
    let f = w.file_end().is_none();
    let wv = Segment::new(PT_LOAD, 0, 0, 0x10, 0x1000, u64::MAX - 15);
    let g = wv.virt_end().is_none();
    a && b && c && e && f && g
}

/// 闸门次序。
fn order_ok() -> bool {
    let ok = Segment::new(PT_LOAD, PROT_RWX, 0, 0x1000, 0x2000, 0x400000);
    let a = ok.check(10000) == Ok(());
    let b = Judgement::of(&ok, 10000).mapped();
    let size = Segment::new(PT_LOAD, 0, 0x1000, 0x2000, 0x1000, 0x400000);
    let c = size.check(10000) == Err(LD_ESIZE);
    let off = Segment::new(PT_LOAD, 0, 0x9000, 0x2000, 0x2000, 0x400000);
    let d = off.check(10000) == Err(LD_EOFF);
    let ty = Segment::new(PT_NOTE, 0, 0x1000, 0x10, 0x10, 0x400000);
    let e = ty.check(10000) == Err(LD_ETYPE);
    let w = Segment::new(PT_LOAD, 0, u64::MAX - 15, 0x1000, 0x1000, 0x400000);
    let f = w.check(u64::MAX) == Err(LD_EWRAP);
    let wv = Segment::new(PT_LOAD, 0, 0x1000, 0x10, 0x1000, u64::MAX - 15);
    let g = wv.check(10000) == Err(LD_EWRAP);
    a && b && c && d && e && f && g
}

/// 页换算。
fn page_ok() -> bool {
    let s = Segment::new(PT_LOAD, PROT_RWX, 0, 0x1234, 0x2345, 0x401234);
    let a = match s.page_span() {
        Ok((start, end)) => start == 0x401000 && end == 0x404000,
        Err(_) => false,
    };
    let e = Segment::new(PT_LOAD, 0, 0, 0, 0, 0x5000);
    let b = e.page_span() == Ok((0x5000, 0x5000));
    let z = Segment::new(PT_LOAD, 0, 0, 0, 0, 0);
    let c = z.page_span() == Ok((0, 0));
    let w = Segment::new(PT_LOAD, 0, 0, 0x10, PAGE, u64::MAX - 15);
    let d = w.page_span() == Err(LD_EWRAP);
    a && b && c && d
}

/// 判定与文案。
fn judge_ok() -> bool {
    let s = Segment::new(PT_LOAD, PROT_RWX, 0x1000, 0x100, 0x1000, 0x400000);
    let j = Judgement::of(&s, 10000);
    let a = j.mapped() && !j.empty() && !j.rejected() && j.end() == 0x1100 && j.bss() == 0xf00;
    let b = message(&s, 10000).starts_with("ok: PT_LOAD map off=4096");
    let z = Segment::new(PT_LOAD, 0, 0, 0, 0x1000, 0x600000);
    let jz = Judgement::of(&z, 10000);
    let c = jz.empty() && message(&z, 10000).starts_with("ok: PT_LOAD bss-only");
    let bad = Segment::new(PT_LOAD, 0, 0x9000, 0x2000, 0x2000, 0x400000);
    let m = message(&bad, 10000);
    let d = m.contains("pt_load off=36864 filesz=8192 past file=10000");
    let e = m.contains("cannot execute binary file: Exec format error");
    let t = Segment::new(PT_DYNAMIC, 0, 0x1000, 0x10, 0x10, 0x400000);
    let f = message(&t, 10000).contains("type=2 not PT_LOAD");
    a && b && c && d && e && f
}

/// 语料全跑。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.total() == CORPUS && r.map() == 5 && r.empty() == 2;
    let b = r.reject() == 7 && r.matched() == CORPUS && r.rate() == 1000 && r.green();
    a && b
}

/// 语料编号。
fn slots_ok() -> bool {
    let mut i = 0usize;
    let mut ok = true;
    while i < CORPUS {
        let s = match CORPUS14.get(i) {
            Some(v) => v,
            None => return false,
        };
        ok = ok && s.no as usize == i + 1;
        ok = ok && s.tag.len() <= NAME_CAP && !s.tag.is_empty();
        ok = ok && s.hit();
        let mut j = i + 1;
        while j < CORPUS {
            if let Some(t) = CORPUS14.get(j) {
                ok = ok && s.tag != t.tag;
            }
            j += 1;
        }
        i += 1;
    }
    let d = corpus_no(0).err() == Some(LD_ENOBS) && corpus_no(15).err() == Some(LD_ENOBS);
    ok && d
}

/// 汇总渲染。
fn report_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.render().as_str()
        == "ptload corpus=14 map=5 empty=2 reject=7 match=14 rate=1000ppm\n";
    let e = Report::new();
    let b = e.is_empty() && e.rate() == 0 && e.gate() == Err(LD_ENOBS);
    let c = e.render().as_str()
        == "ptload corpus=0 map=0 empty=0 reject=0 match=0 rate=0ppm\n";
    a && b && c
}

/// 红单。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, LD_ETYPE))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched() == CORPUS - 1 && r.rate() == 929;
    let b = r.gate() == Err(LD_EVER) && !r.green();
    a && b
}

/// 覆盖档。
fn cover_ok() -> bool {
    cover(CORPUS) == Ok(()) && cover(CORPUS - 1) == Err(LD_ENOBS) && cover(0) == Err(LD_ENOBS)
}

/// 映射器与错误码。
fn mapper_ok() -> bool {
    let s = Segment::new(PT_LOAD, PROT_RWX, 0x1000, 0x100, 0x1000, 0x400000);
    let mut mp = Mapper::new();
    let a = !mp.bound() && mp.map(&s, 10000) == Ok(()) && mp.bound();
    let b = mp.start() == 0x400000 && mp.end() == 0x401000;
    let c = mp.map(&s, 10000) == Err(LD_ESTATE);
    let bad = Segment::new(PT_LOAD, 0, 0x9000, 0x2000, 0x2000, 0x400000);
    let mut mp2 = Mapper::new();
    let d = mp2.map(&bad, 10000) == Err(LD_EOFF) && !mp2.bound();
    let e = ENOEXEC == 8 && errno(&s, 10000) == 0 && errno(&bad, 10000) == ENOEXEC;
    a && b && c && d && e
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && arith_ok();
    out[1] = order_ok() && page_ok();
    out[2] = judge_ok() && corpus_ok() && slots_ok();
    out[3] = report_ok() && reds_ok() && cover_ok() && mapper_ok()
        && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11604_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11604.ptload_bounds");

    s.add(
        "LD01-名称闸门",
        {
            name_ok()
        },
        "static",
    );

    s.add(
        "LD02-段算术",
        {
            arith_ok()
        },
        "static",
    );

    s.add(
        "LD03-闸门次序",
        {
            order_ok()
        },
        "static",
    );

    s.add(
        "LD04-页换算",
        {
            page_ok()
        },
        "static",
    );

    s.add(
        "LD05-判定与文案",
        {
            judge_ok()
        },
        "static",
    );

    s.add(
        "LD06-十四卡全跑",
        {
            corpus_ok()
        },
        "static",
    );

    s.add(
        "LD07-语料编号",
        {
            slots_ok()
        },
        "static",
    );

    s.add(
        "LD08-汇总渲染",
        {
            report_ok()
        },
        "static",
    );

    s.add(
        "LD09-红单显式",
        {
            reds_ok()
        },
        "static",
    );

    s.add(
        "LD10-覆盖档",
        {
            cover_ok()
        },
        "static",
    );

    s.add(
        "LD11-一次性映射器",
        {
            mapper_ok()
        },
        "static",
    );

    s.add(
        "LD12-账本对账",
        {
            build_ledger().audit() == Ok(())
        },
        "static",
    );

    s.add(
        "LD13-冻结重放四注入",
        {
            let four = inject_four();
            four[0] && four[1] && four[2] && four[3]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_gates() {
        assert_eq!(Name::new(b"").err(), Some(LD_EBYTE));
        assert_eq!(Name::new(&[0x7f]).err(), Some(LD_EBYTE));
        assert_eq!(Name::new(&[0u8; 17]).err(), Some(LD_EBYTE));
        let n = Name::new(b"end_exact").expect("n");
        assert_eq!(n.len(), 9);
        assert!(n.same(b"end_exact"));
    }

    #[test]
    fn segment_math() {
        let s = Segment::new(PT_LOAD, PROT_RWX, 0x1000, 0x1000, 0x2000, 0x400000);
        assert_eq!(s.file_end(), Some(0x2000));
        assert_eq!(s.virt_end(), Some(0x402000));
        assert_eq!(s.bss_len(), 0x1000);
        assert!(!s.bss_only());
        assert!(s.is_load());
        let w = Segment::new(PT_LOAD, 0, u64::MAX - 15, 0x1000, 0x1000, 0);
        assert_eq!(w.file_end(), None);
    }

    #[test]
    fn check_chain() {
        let ok = Segment::new(PT_LOAD, 7, 0, 0x1000, 0x2000, 0x400000);
        assert_eq!(ok.check(10000), Ok(()));
        assert_eq!(
            Segment::new(PT_LOAD, 0, 0x1000, 0x2000, 0x1000, 0).check(10000),
            Err(LD_ESIZE)
        );
        assert_eq!(
            Segment::new(PT_LOAD, 0, 0x9000, 0x2000, 0x2000, 0).check(10000),
            Err(LD_EOFF)
        );
        assert_eq!(
            Segment::new(PT_NOTE, 0, 0x1000, 0x10, 0x10, 0).check(10000),
            Err(LD_ETYPE)
        );
    }

    #[test]
    fn wrap_checks() {
        let w = Segment::new(PT_LOAD, 0, u64::MAX - 15, 0x1000, 0x1000, 0);
        assert_eq!(w.check(u64::MAX), Err(LD_EWRAP));
        let wv = Segment::new(PT_LOAD, 0, 0x1000, 0x10, 0x1000, u64::MAX - 15);
        assert_eq!(wv.check(10000), Err(LD_EWRAP));
    }

    #[test]
    fn page_span() {
        let s = Segment::new(PT_LOAD, 7, 0, 0x1234, 0x2345, 0x401234);
        assert_eq!(s.page_span(), Ok((0x401000, 0x404000)));
        let z = Segment::new(PT_LOAD, 0, 0, 0, 0, 0);
        assert_eq!(z.page_span(), Ok((0, 0)));
        assert_eq!(page_down(0x1234), 0x1000);
    }

    #[test]
    fn judgement_states() {
        let m = Segment::new(PT_LOAD, 7, 0x1000, 0x100, 0x1000, 0x400000);
        let j = Judgement::of(&m, 10000);
        assert!(j.mapped());
        assert_eq!(j.bss(), 0xf00);
        let e = Segment::new(PT_LOAD, 0, 0, 0, 0x1000, 0x600000);
        assert!(Judgement::of(&e, 10000).empty());
        let r = Segment::new(PT_DYNAMIC, 0, 0x1000, 0x10, 0x10, 0x400000);
        let k = Judgement::of(&r, 10000);
        assert!(k.rejected());
        assert_eq!(k.code(), LD_ETYPE);
    }

    #[test]
    fn message_texts() {
        let s = Segment::new(PT_LOAD, 7, 0x1000, 0x100, 0x1000, 0x400000);
        assert!(message(&s, 10000).starts_with("ok: PT_LOAD map off=4096 filesz=256"));
        let z = Segment::new(PT_LOAD, 0, 0, 0, 0x1000, 0x600000);
        assert!(message(&z, 10000).starts_with("ok: PT_LOAD bss-only memsz=4096"));
        let bad = Segment::new(PT_LOAD, 0, 0x9000, 0x2000, 0x2000, 0);
        assert!(message(&bad, 10000).contains("past file=10000"));
        let sz = Segment::new(PT_LOAD, 0, 0x1000, 0x2000, 0x1000, 0);
        assert!(message(&sz, 10000).contains("filesz=8192 > memsz=4096"));
    }

    #[test]
    fn corpus_counts() {
        let r = run().expect("r");
        assert_eq!(r.total(), CORPUS);
        assert_eq!(r.map(), 5);
        assert_eq!(r.empty(), 2);
        assert_eq!(r.reject(), 7);
        assert_eq!(r.matched(), CORPUS);
        assert_eq!(r.rate(), 1000);
        assert_eq!(r.gate(), Ok(()));
        assert_eq!(
            r.render().as_str(),
            "ptload corpus=14 map=5 empty=2 reject=7 match=14 rate=1000ppm\n"
        );
    }

    #[test]
    fn red_rate() {
        let r = run_with(Some((0, LD_ETYPE))).expect("r");
        assert_eq!(r.matched(), 13);
        assert_eq!(r.rate(), 929);
        assert_eq!(r.gate(), Err(LD_EVER));
    }

    #[test]
    fn empty_report() {
        let r = Report::new();
        assert!(r.is_empty());
        assert_eq!(r.rate(), 0);
        assert_eq!(r.gate(), Err(LD_ENOBS));
    }

    #[test]
    fn coverage_gate() {
        assert_eq!(cover(14), Ok(()));
        assert_eq!(cover(13), Err(LD_ENOBS));
    }

    #[test]
    fn mapper_once() {
        let s = Segment::new(PT_LOAD, 7, 0x1000, 0x100, 0x1000, 0x400000);
        let mut mp = Mapper::new();
        assert_eq!(mp.map(&s, 10000), Ok(()));
        assert_eq!(mp.start(), 0x400000);
        assert_eq!(mp.end(), 0x401000);
        assert_eq!(mp.map(&s, 10000), Err(LD_ESTATE));
    }

    #[test]
    fn corpus_lookup() {
        assert_eq!(corpus_no(0).err(), Some(LD_ENOBS));
        assert_eq!(corpus_no(15).err(), Some(LD_ENOBS));
        assert_eq!(corpus_no(12).expect("s").tag, b"end_exact");
        assert!(corpus_no(13).expect("s").hit());
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.sample(7).map(|s| s.value), Some(929));
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = l.freeze().expect("freeze");
        assert!(l.is_frozen());
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze().err(), Some(LD_EBAL));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn injections() {
        let four = inject_four();
        assert!(four[0] && four[1] && four[2] && four[3]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11604_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }
}
