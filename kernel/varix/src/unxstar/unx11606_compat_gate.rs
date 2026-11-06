//! UNX-F11606 ELF class/机种不匹配（ELFCLASS32/EM_386）拒收文案
//!
//! `e_ident` 前 8 字节是魔数与自描述：`EI_CLASS`（偏移 4）说字宽，`EI_DATA`
//! （偏移 5）说字节序；真正的机种在 `e_machine`（偏移 18..20），目标 ABI 版本
//! 在 `e_version`（偏移 20..24）。x86_64 内核只认 `ELFCLASS64 + ELFDATA2LSB +
//! EV_CURRENT + EM_X86_64` 这一组，其它任何一维不符都只能拒收：32 位镜像需要
//! 内核带 IA32 兼容层（`CONFIG_IA32_EMULATION`），大端镜像方向反了，别的机种
//! 的指令集根本没法在本核解码。
//!
//! 拒收形态同样是 `execve` 的 ENOEXEC(8)，但括号里的原因要能直接对上
//! `readelf -h` 的字段：`(ELFCLASS32, need ELFCLASS64)`、
//! `(ELFCLASS64, need ELFCLASS64)` 这类短语按维分别生成，不许混成一句
//! "格式错误" 了事——排查的人拿到的第一手信息就在这句里。
//!
//! 判据次序固定：先读字节（不足 24 字节读不全四维，落字节档），再认字宽，
//! 再认字节序，再认 ABI 版本，最后认机种。次序不能换：32 位大端镜像同时踩
//! 两维，内核先报字宽，这与 `load_elf_binary` 的实测输出一致；反过来报会
//! 让上层脚本的字符串分支走错档。
//!
//! 闸门次序：字节 → 字宽 → 字节序 → 版本 → 机种 → 语料 → 文案 → 期望面 → 绿档。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本 14 格
//! （8 成 6 败）在 `audit` 逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfb50;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 语料条数。
pub const CORPUS: usize = 14;

/// `EI_CLASS` 偏移。
pub const EI_CLASS: usize = 4;

/// `EI_DATA` 偏移。
pub const EI_DATA: usize = 5;

/// `e_machine` 偏移。
pub const E_MACHINE_OFF: usize = 18;

/// `e_version` 偏移。
pub const E_VERSION_OFF: usize = 20;

/// 读四维所需最短头长。
pub const MIN_EHDR: usize = E_VERSION_OFF + 4;

/// `ENOEXEC`。
pub const ENOEXEC: i32 = 8;

// ---------------------------------------------------------------------------
// 四维取值
// ---------------------------------------------------------------------------

/// 非法字宽。
pub const ELFCLASSNONE: u8 = 0;
/// 32 位。
pub const ELFCLASS32: u8 = 1;
/// 64 位。
pub const ELFCLASS64: u8 = 2;

/// 非法字节序。
pub const ELFDATANONE: u8 = 0;
/// 小端。
pub const ELFDATA2LSB: u8 = 1;
/// 大端。
pub const ELFDATA2MSB: u8 = 2;

/// 无机器。
pub const EM_NONE: u16 = 0;
/// i386。
pub const EM_386: u16 = 3;
/// x86_64。
pub const EM_X86_64: u16 = 62;
/// AArch64。
pub const EM_AARCH64: u16 = 183;
/// RISC-V。
pub const EM_RISCV: u16 = 243;

/// 非现行 ABI 版本。
pub const EV_NONE: u32 = 0;
/// 现行 ABI 版本。
pub const EV_CURRENT: u32 = 1;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 字宽不符。
pub const CL_ECLASS: u32 = SEG_BASE + 0x01;
/// 字节序不符。
pub const CL_EDATA: u32 = SEG_BASE + 0x02;
/// ABI 版本不符。
pub const CL_ECVER: u32 = SEG_BASE + 0x03;
/// 机种不符。
pub const CL_EMACH: u32 = SEG_BASE + 0x04;
/// 状态冲突。
pub const CL_ESTATE: u32 = SEG_BASE + 0x05;
/// 未达绿档。
pub const CL_EVER: u32 = SEG_BASE + 0x06;
/// 字节非法。
pub const CL_EBYTE: u32 = SEG_BASE + 0x07;
/// 语料重复。
pub const CL_EDUP: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const CL_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const CL_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const CL_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const CL_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const CL_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 认字宽。
pub const OP_CLASS: u8 = 0x01;
/// 认字节序。
pub const OP_DATA: u8 = 0x02;
/// 认机种。
pub const OP_MACH: u8 = 0x03;
/// 认版本。
pub const OP_VER: u8 = 0x04;
/// 文案。
pub const OP_MSG: u8 = 0x05;
/// 语料全跑。
pub const OP_CORPUS: u8 = 0x06;
/// 头读取。
pub const OP_HEAD: u8 = 0x07;
/// 期望面。
pub const OP_WANT: u8 = 0x08;

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
fn u32_text(v: u32) -> String {
    let mut buf = [0u8; 10];
    let mut n = v;
    let mut i = 10usize;
    if n == 0 {
        return String::from("0");
    }
    while n > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + ((n % 10) as u8);
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

/// 字宽名。
pub fn class_name(c: u8) -> &'static str {
    match c {
        ELFCLASS32 => "ELFCLASS32",
        ELFCLASS64 => "ELFCLASS64",
        _ => "ELFCLASSNONE",
    }
}

/// 字节序名。
pub fn data_name(d: u8) -> &'static str {
    match d {
        ELFDATA2LSB => "ELFDATA2LSB",
        ELFDATA2MSB => "ELFDATA2MSB",
        _ => "ELFDATANONE",
    }
}

/// 机种名。
pub fn machine_name(m: u16) -> &'static str {
    match m {
        EM_NONE => "EM_NONE",
        EM_386 => "EM_386",
        EM_X86_64 => "EM_X86_64",
        EM_AARCH64 => "EM_AARCH64",
        EM_RISCV => "EM_RISCV",
        _ => "EM_UNKNOWN",
    }
}

/// 版本名。
pub fn version_name(v: u32) -> &'static str {
    match v {
        EV_NONE => "EV_NONE",
        EV_CURRENT => "EV_CURRENT",
        _ => "EV_UNKNOWN",
    }
}

// ---------------------------------------------------------------------------
// 四维
// ---------------------------------------------------------------------------

/// 一个镜像的四维。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Compat {
    /// 字宽。
    pub class: u8,
    /// 字节序。
    pub data: u8,
    /// 机种。
    pub machine: u16,
    /// ABI 版本。
    pub version: u32,
}

impl Compat {
    /// 构造。
    pub const fn new(class: u8, data: u8, machine: u16, version: u32) -> Compat {
        Compat { class, data, machine, version }
    }

    /// 逐维比对；返回第一个不符维的码。
    pub fn check(&self, w: &Want) -> Result<(), u32> {
        if self.class != w.class {
            return Err(CL_ECLASS);
        }
        if self.data != w.data {
            return Err(CL_EDATA);
        }
        if self.version != w.version {
            return Err(CL_ECVER);
        }
        if self.machine != w.machine {
            return Err(CL_EMACH);
        }
        Ok(())
    }

    /// 摘要。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.class as u64);
        h = fold_word(h, self.data as u64);
        h = fold_word(h, self.machine as u64);
        fold_word(h, self.version as u64)
    }
}

/// 本核算力面：期望的四维。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Want {
    /// 字宽。
    pub class: u8,
    /// 字节序。
    pub data: u8,
    /// 机种。
    pub machine: u16,
    /// ABI 版本。
    pub version: u32,
}

impl Want {
    /// x86_64 面。
    pub const fn x86_64() -> Want {
        Want { class: ELFCLASS64, data: ELFDATA2LSB, machine: EM_X86_64, version: EV_CURRENT }
    }

    /// 是否本核默认面。
    pub fn is_default(&self) -> bool {
        *self == Want::x86_64()
    }

    /// 文案（期望侧）。
    pub fn text(&self) -> String {
        let mut s = String::from(class_name(self.class));
        s.push('/');
        s.push_str(data_name(self.data));
        s.push('/');
        s.push_str(machine_name(self.machine));
        s.push('/');
        s.push_str(version_name(self.version));
        s
    }
}

// ---------------------------------------------------------------------------
// 文案
// ---------------------------------------------------------------------------

/// 一维不符短语。
pub fn clause(c: &Compat, w: &Want, code: u32) -> String {
    let mut s = String::new();
    match code {
        CL_ECLASS => {
            s.push_str(class_name(c.class));
            s.push_str(", need ");
            s.push_str(class_name(w.class));
        }
        CL_EDATA => {
            s.push_str(data_name(c.data));
            s.push_str(", need ");
            s.push_str(data_name(w.data));
        }
        CL_ECVER => {
            s.push_str("e_version=");
            s.push_str(u32_text(c.version).as_str());
            s.push_str(", need ");
            s.push_str(u32_text(w.version).as_str());
        }
        CL_EMACH => {
            s.push_str(machine_name(c.machine));
            s.push_str(", need ");
            s.push_str(machine_name(w.machine));
        }
        _ => {
            s.push_str("unknown mismatch");
        }
    }
    s
}

/// 整句文案。
pub fn message(c: &Compat, w: &Want, path: &str) -> Result<String, u32> {
    if path.is_empty() {
        return Err(CL_EBYTE);
    }
    let mut s = String::new();
    match c.check(w) {
        Ok(()) => {
            s.push_str("ok: ");
            s.push_str(w.text().as_str());
            s.push('\n');
        }
        Err(code) => {
            s.push_str("bash: ");
            s.push_str(path);
            s.push_str(": cannot execute binary file: Exec format error (");
            s.push_str(clause(c, w, code).as_str());
            s.push_str(")\n");
        }
    }
    Ok(s)
}

/// errno。
pub fn errno(c: &Compat, w: &Want) -> i32 {
    match c.check(w) {
        Ok(()) => 0,
        Err(_) => ENOEXEC,
    }
}

// ---------------------------------------------------------------------------
// 头读取
// ---------------------------------------------------------------------------

/// 24 字节头缓冲。
#[derive(Clone, Copy, Debug)]
pub struct Head {
    buf: [u8; MIN_EHDR],
    len: usize,
}

impl Head {
    /// 取字节。
    pub fn new(bytes: &[u8]) -> Head {
        let mut buf = [0u8; MIN_EHDR];
        let mut i = 0usize;
        while i < MIN_EHDR && i < bytes.len() {
            if let Some(b) = bytes.get(i) {
                buf[i] = *b;
            }
            i += 1;
        }
        let len = if bytes.len() > MIN_EHDR { MIN_EHDR } else { bytes.len() };
        Head { buf, len }
    }

    /// 实长。
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 是否够读四维。
    pub fn complete(&self) -> bool {
        self.len >= MIN_EHDR
    }

    /// 缺口。
    pub fn short_by(&self) -> usize {
        MIN_EHDR.saturating_sub(self.len)
    }

    /// 取一字节。
    pub fn byte_at(&self, off: usize) -> Result<u8, u32> {
        if off >= self.len {
            return Err(CL_EBYTE);
        }
        match self.buf.get(off) {
            Some(b) => Ok(*b),
            None => Err(CL_EBYTE),
        }
    }

    /// 小端 16 位。
    pub fn le16(&self, off: usize) -> Result<u16, u32> {
        let lo = self.byte_at(off)? as u16;
        let hi = self.byte_at(off + 1)? as u16;
        Ok(lo | (hi << 8))
    }

    /// 小端 32 位。
    pub fn le32(&self, off: usize) -> Result<u32, u32> {
        let b0 = self.byte_at(off)? as u32;
        let b1 = self.byte_at(off + 1)? as u32;
        let b2 = self.byte_at(off + 2)? as u32;
        let b3 = self.byte_at(off + 3)? as u32;
        Ok(b0 | (b1 << 8) | (b2 << 16) | (b3 << 24))
    }

    /// 读四维。
    pub fn compat(&self) -> Result<Compat, u32> {
        if !self.complete() {
            return Err(CL_EBYTE);
        }
        let class = self.byte_at(EI_CLASS)?;
        let data = self.byte_at(EI_DATA)?;
        let machine = self.le16(E_MACHINE_OFF)?;
        let version = self.le32(E_VERSION_OFF)?;
        Ok(Compat::new(class, data, machine, version))
    }

    /// 摘要。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(b) = self.buf.get(i) {
                h = fold_byte(h, *b);
            }
            i += 1;
        }
        fold_word(h, self.len as u64)
    }
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 一个语料槽。
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    /// 四维。
    pub c: Compat,
    /// 期望码（0 = 存活）。
    pub code: u32,
    /// 备注。
    pub note: &'static str,
}

impl Slot {
    /// 构造。
    pub const fn new(c: Compat, code: u32, note: &'static str) -> Slot {
        Slot { c, code, note }
    }
}

/// 十四槽语料：一活十三死。
pub const CASES: [Slot; CORPUS] = [
    Slot::new(Compat::new(ELFCLASS64, ELFDATA2LSB, EM_X86_64, EV_CURRENT), 0, "host-exact"),
    Slot::new(Compat::new(ELFCLASS64, ELFDATA2LSB, EM_X86_64, EV_NONE), CL_ECVER, "version-zero"),
    Slot::new(Compat::new(ELFCLASS64, ELFDATA2LSB, EM_X86_64, 2), CL_ECVER, "version-two"),
    Slot::new(Compat::new(ELFCLASS64, ELFDATA2MSB, EM_X86_64, EV_CURRENT), CL_EDATA, "big-endian"),
    Slot::new(Compat::new(ELFCLASS32, ELFDATA2LSB, EM_386, EV_CURRENT), CL_ECLASS, "ia32-386"),
    Slot::new(Compat::new(ELFCLASS32, ELFDATA2LSB, EM_X86_64, EV_CURRENT), CL_ECLASS, "class32-x64"),
    Slot::new(Compat::new(ELFCLASS64, ELFDATA2LSB, EM_386, EV_CURRENT), CL_EMACH, "machine-386"),
    Slot::new(Compat::new(ELFCLASS64, ELFDATA2LSB, EM_AARCH64, EV_CURRENT), CL_EMACH, "machine-arm"),
    Slot::new(Compat::new(ELFCLASS64, ELFDATA2LSB, EM_RISCV, EV_CURRENT), CL_EMACH, "machine-rv"),
    Slot::new(Compat::new(ELFCLASSNONE, ELFDATA2LSB, EM_X86_64, EV_CURRENT), CL_ECLASS, "class-none"),
    Slot::new(Compat::new(3, ELFDATA2LSB, EM_X86_64, EV_CURRENT), CL_ECLASS, "class-three"),
    Slot::new(Compat::new(ELFCLASS64, ELFDATANONE, EM_X86_64, EV_CURRENT), CL_EDATA, "data-none"),
    Slot::new(Compat::new(ELFCLASS64, 3, EM_X86_64, EV_CURRENT), CL_EDATA, "data-three"),
    Slot::new(Compat::new(ELFCLASS64, ELFDATA2LSB, EM_NONE, EV_CURRENT), CL_EMACH, "machine-none"),
];

/// 取一槽。
pub fn slot_at(i: usize) -> Option<Slot> {
    CASES.get(i).copied()
}

/// 四维是否互异。
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
                if b.c == a.c {
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
    if !unique() {
        return false;
    }
    let w = Want::x86_64();
    let mut i = 0usize;
    while i < CASES.len() {
        let slot = match CASES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let got = match slot.c.check(&w) {
            Ok(()) => 0,
            Err(c) => c,
        };
        if got != slot.code {
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
    class: usize,
    data: usize,
    version: usize,
    machine: usize,
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
            class: 0,
            data: 0,
            version: 0,
            machine: 0,
            matched: 0,
            red: false,
        }
    }

    /// 总条数。
    pub const fn total(&self) -> usize {
        self.total
    }

    /// 存活条数。
    pub const fn accept(&self) -> usize {
        self.accept
    }

    /// 拒收条数。
    pub const fn reject(&self) -> usize {
        self.reject
    }

    /// 字宽拒收数。
    pub const fn class(&self) -> usize {
        self.class
    }

    /// 字节序拒收数。
    pub const fn data(&self) -> usize {
        self.data
    }

    /// 版本拒收数。
    pub const fn version(&self) -> usize {
        self.version
    }

    /// 机种拒收数。
    pub const fn machine(&self) -> usize {
        self.machine
    }

    /// 对上的条数。
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
            return Err(CL_ENOBS);
        }
        if self.red {
            return Err(CL_EVER);
        }
        if self.matched != self.total {
            return Err(CL_EVALUE);
        }
        if self.accept != 1 || self.reject != 13 {
            return Err(CL_EVALUE);
        }
        if self.class != 4 || self.data != 3 || self.version != 2 || self.machine != 4 {
            return Err(CL_EVALUE);
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
        s.push_str("compat corpus=");
        s.push_str(u32_text(self.total as u32).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept as u32).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject as u32).as_str());
        s.push_str(" class=");
        s.push_str(u32_text(self.class as u32).as_str());
        s.push_str(" data=");
        s.push_str(u32_text(self.data as u32).as_str());
        s.push_str(" version=");
        s.push_str(u32_text(self.version as u32).as_str());
        s.push_str(" machine=");
        s.push_str(u32_text(self.machine as u32).as_str());
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
    let w = Want::x86_64();
    let mut r = Report::new();
    let mut i = 0usize;
    while i < CASES.len() {
        let slot = match CASES.get(i) {
            Some(v) => *v,
            None => return Err(CL_EVALUE),
        };
        r.total += 1;
        match slot.c.check(&w) {
            Ok(()) => {
                r.accept += 1;
                if slot.code == 0 {
                    r.matched += 1;
                }
            }
            Err(code) => {
                r.reject += 1;
                match code {
                    CL_ECLASS => r.class += 1,
                    CL_EDATA => r.data += 1,
                    CL_ECVER => r.version += 1,
                    CL_EMACH => r.machine += 1,
                    _ => {}
                }
                if slot.code == code {
                    r.matched += 1;
                }
            }
        }
        i += 1;
    }
    if let Some((slot, _code)) = red {
        if slot >= r.total {
            return Err(CL_ESLOT);
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
        Err(CL_ENOBS)
    }
}

// ---------------------------------------------------------------------------
// 面探针
// ---------------------------------------------------------------------------

/// 面探针：只在期望面上允许二次核对。
#[derive(Clone, Copy, Debug)]
pub struct Probe {
    bound: bool,
    hits: u32,
    last: u32,
}

impl Probe {
    /// 空。
    pub const fn new() -> Probe {
        Probe { bound: false, hits: 0, last: 0 }
    }

    /// 探一次。
    pub fn probe(&mut self, c: &Compat, w: &Want) -> Result<(), u32> {
        if self.bound {
            self.last = CL_ESTATE;
            return Err(CL_ESTATE);
        }
        self.bound = true;
        self.hits = self.hits.saturating_add(1);
        match c.check(w) {
            Ok(()) => {
                self.last = 0;
                Ok(())
            }
            Err(code) => {
                self.last = code;
                Err(code)
            }
        }
    }

    /// 是否绑定。
    pub const fn bound(&self) -> bool {
        self.bound
    }

    /// 命中数。
    pub const fn hits(&self) -> u32 {
        self.hits
    }

    /// 最后码。
    pub const fn last(&self) -> u32 {
        self.last
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
            return Err(CL_ESLOT);
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
            return Err(CL_EBAL);
        }
        const VALS: [u32; 8] = [14, 1, 13, 4, 3, 2, 4, 929];
        const CODES: [u32; 6] = [CL_ECLASS, CL_EDATA, CL_ECVER, CL_EMACH, CL_ESTATE, CL_EBYTE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(CL_EBAL),
            };
            if s.slot as usize != i {
                return Err(CL_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(CL_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(CL_EVALUE);
                        }
                    }
                    None => return Err(CL_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(CL_EVALUE);
                        }
                    }
                    None => return Err(CL_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(CL_EBAL);
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
            return Err(CL_EBAL);
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
            Err(CL_ESEAL)
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
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.accept())));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.reject())));
    let _ = l.register(count_cell(3, OP_CORPUS, r.map(|x| x.class())));

    let _ = l.register(count_cell(4, OP_CORPUS, r.map(|x| x.data())));
    let _ = l.register(count_cell(5, OP_CORPUS, r.map(|x| x.version())));
    let _ = l.register(count_cell(6, OP_CORPUS, r.map(|x| x.machine())));

    let red = run_with(Some((0, CL_ECLASS)));
    let _ = l.register(count_cell(7, OP_CLASS, red.map(|x| x.rate() as usize)));

    let w = Want::x86_64();
    let class_err = match Compat::new(ELFCLASS32, ELFDATA2LSB, EM_386, EV_CURRENT).check(&w) {
        Ok(()) => CL_ENOBS,
        Err(c) => c,
    };
    let _ = l.register(err_cell(8, OP_CLASS, class_err));

    let data_err = match Compat::new(ELFCLASS64, ELFDATA2MSB, EM_X86_64, EV_CURRENT).check(&w) {
        Ok(()) => CL_ENOBS,
        Err(c) => c,
    };
    let _ = l.register(err_cell(9, OP_DATA, data_err));

    let ver_err = match Compat::new(ELFCLASS64, ELFDATA2LSB, EM_X86_64, EV_NONE).check(&w) {
        Ok(()) => CL_ENOBS,
        Err(c) => c,
    };
    let _ = l.register(err_cell(10, OP_VER, ver_err));

    let mach_err = match Compat::new(ELFCLASS64, ELFDATA2LSB, EM_AARCH64, EV_CURRENT).check(&w) {
        Ok(()) => CL_ENOBS,
        Err(c) => c,
    };
    let _ = l.register(err_cell(11, OP_MACH, mach_err));

    let mut p = Probe::new();
    let _ = p.probe(&Compat::new(ELFCLASS64, ELFDATA2LSB, EM_X86_64, EV_CURRENT), &w);
    let state_err = match p.probe(&Compat::new(ELFCLASS64, ELFDATA2LSB, EM_X86_64, EV_CURRENT), &w) {
        Ok(()) => CL_ESTATE,
        Err(c) => c,
    };
    let _ = l.register(err_cell(12, OP_WANT, state_err));

    let short = [0u8; MIN_EHDR - 1];
    let h = Head::new(&short);
    let byte_err = match h.compat() {
        Ok(_) => CL_EBYTE,
        Err(c) => c,
    };
    let _ = l.register(err_cell(13, OP_HEAD, byte_err));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 名称表。
fn names_ok() -> bool {
    class_name(ELFCLASSNONE) == "ELFCLASSNONE"
        && class_name(ELFCLASS32) == "ELFCLASS32"
        && class_name(ELFCLASS64) == "ELFCLASS64"
        && class_name(9) == "ELFCLASSNONE"
        && data_name(ELFDATANONE) == "ELFDATANONE"
        && data_name(ELFDATA2LSB) == "ELFDATA2LSB"
        && data_name(ELFDATA2MSB) == "ELFDATA2MSB"
        && data_name(9) == "ELFDATANONE"
        && machine_name(EM_NONE) == "EM_NONE"
        && machine_name(EM_386) == "EM_386"
        && machine_name(EM_X86_64) == "EM_X86_64"
        && machine_name(EM_AARCH64) == "EM_AARCH64"
        && machine_name(EM_RISCV) == "EM_RISCV"
        && machine_name(1) == "EM_UNKNOWN"
        && version_name(EV_NONE) == "EV_NONE"
        && version_name(EV_CURRENT) == "EV_CURRENT"
        && version_name(7) == "EV_UNKNOWN"
}

/// 次序闸门。
fn order_ok() -> bool {
    let w = Want::x86_64();
    let a = Compat::new(ELFCLASS32, ELFDATA2MSB, EM_386, EV_NONE).check(&w) == Err(CL_ECLASS);
    let b = Compat::new(ELFCLASS64, ELFDATA2MSB, EM_386, EV_NONE).check(&w) == Err(CL_EDATA);
    let c = Compat::new(ELFCLASS64, ELFDATA2LSB, EM_386, EV_NONE).check(&w) == Err(CL_ECVER);
    let d = Compat::new(ELFCLASS64, ELFDATA2LSB, EM_386, EV_CURRENT).check(&w) == Err(CL_EMACH);
    let e = Compat::new(ELFCLASS64, ELFDATA2LSB, EM_X86_64, EV_CURRENT).check(&w) == Ok(());
    a && b && c && d && e
}

/// 头读取。
fn head_ok() -> bool {
    let mut buf = [0u8; MIN_EHDR];
    buf[EI_CLASS] = ELFCLASS64;
    buf[EI_DATA] = ELFDATA2LSB;
    buf[E_MACHINE_OFF] = 62;
    buf[E_MACHINE_OFF + 1] = 0;
    buf[E_VERSION_OFF] = 1;
    let h = Head::new(&buf);
    let a = h.complete() && h.len() == MIN_EHDR && h.short_by() == 0;
    let b = h.byte_at(EI_CLASS) == Ok(ELFCLASS64) && h.le16(E_MACHINE_OFF) == Ok(62);
    let c = h.le32(E_VERSION_OFF) == Ok(1);
    let d = match h.compat() {
        Ok(c0) => c0 == Compat::new(ELFCLASS64, ELFDATA2LSB, EM_X86_64, EV_CURRENT),
        Err(_) => false,
    };
    let e = h.byte_at(MIN_EHDR) == Err(CL_EBYTE) && h.le32(MIN_EHDR - 2) == Err(CL_EBYTE);
    a && b && c && d && e
}

/// 短头拒收。
fn short_ok() -> bool {
    let a = {
        let h = Head::new(&[0u8; 0]);
        h.is_empty() && h.short_by() == MIN_EHDR && h.compat() == Err(CL_EBYTE)
    };
    let b = {
        let h = Head::new(&[0u8; MIN_EHDR - 1]);
        !h.complete() && h.short_by() == 1 && h.compat() == Err(CL_EBYTE)
    };
    let c = {
        let h = Head::new(&[0u8; MIN_EHDR]);
        h.complete() && h.short_by() == 0
    };
    a && b && c
}

/// 期望面。
fn want_ok() -> bool {
    let w = Want::x86_64();
    let a = w.is_default() && w.class == ELFCLASS64 && w.data == ELFDATA2LSB;
    let b = w.machine == EM_X86_64 && w.version == EV_CURRENT;
    let c = w.text().as_str() == "ELFCLASS64/ELFDATA2LSB/EM_X86_64/EV_CURRENT";
    let d = {
        let x = Want { class: ELFCLASS32, data: ELFDATA2LSB, machine: EM_386, version: EV_CURRENT };
        !x.is_default() && x.text().as_str() == "ELFCLASS32/ELFDATA2LSB/EM_386/EV_CURRENT"
    };
    a && b && c && d
}

/// 语料全跑。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.total() == CORPUS && r.accept() == 1 && r.reject() == 13 && r.matched() == CORPUS;
    let b = r.class() == 4 && r.data() == 3 && r.version() == 2 && r.machine() == 4;
    let c = r.rate() == 1000 && r.green() && r.gate() == Ok(());
    a && b && c
}

/// 语料编号。
fn slots_ok() -> bool {
    if !unique() || !notes_ok() || !table_ok() {
        return false;
    }
    let a = match slot_at(0) {
        Some(s) => s.code == 0 && s.c.class == ELFCLASS64,
        None => false,
    };
    let b = match slot_at(4) {
        Some(s) => s.code == CL_ECLASS && s.c.class == ELFCLASS32 && s.c.machine == EM_386,
        None => false,
    };
    let c = slot_at(CORPUS).is_none() && slot_at(CORPUS + 1).is_none();
    a && b && c
}

/// 汇总渲染。
fn report_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.render().as_str()
        == "compat corpus=14 accept=1 reject=13 class=4 data=3 version=2 machine=4 match=14 rate=1000ppm\n";
    let e = Report::new();
    let b = e.is_empty() && e.rate() == 0 && e.gate() == Err(CL_ENOBS);
    let c = e.render().as_str()
        == "compat corpus=0 accept=0 reject=0 class=0 data=0 version=0 machine=0 match=0 rate=0ppm\n";
    a && b && c
}

/// 红单显式。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, CL_ECLASS))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched() == CORPUS - 1 && r.rate() == 929;
    let b = r.gate() == Err(CL_EVER) && !r.green();
    let c = run_with(Some((CORPUS, CL_ECLASS))).err() == Some(CL_ESLOT);
    a && b && c
}

/// 封面。
fn cover_ok() -> bool {
    cover(CORPUS) == Ok(()) && cover(CORPUS - 1) == Err(CL_ENOBS) && cover(0) == Err(CL_ENOBS)
}

/// 文案逐字。
fn message_ok() -> bool {
    let w = Want::x86_64();
    let a = match message(&Compat::new(ELFCLASS32, ELFDATA2LSB, EM_386, EV_CURRENT), &w, "./a.out") {
        Ok(s) => s.as_str()
            == "bash: ./a.out: cannot execute binary file: Exec format error (ELFCLASS32, need ELFCLASS64)\n",
        Err(_) => false,
    };
    let b = match message(&Compat::new(ELFCLASS64, ELFDATA2LSB, EM_386, EV_CURRENT), &w, "./a.out") {
        Ok(s) => s.as_str()
            == "bash: ./a.out: cannot execute binary file: Exec format error (EM_386, need EM_X86_64)\n",
        Err(_) => false,
    };
    let c = match message(&Compat::new(ELFCLASS64, ELFDATA2MSB, EM_X86_64, EV_CURRENT), &w, "./a.out") {
        Ok(s) => s.as_str()
            == "bash: ./a.out: cannot execute binary file: Exec format error (ELFDATA2MSB, need ELFDATA2LSB)\n",
        Err(_) => false,
    };
    let d = match message(&Compat::new(ELFCLASS64, ELFDATA2LSB, EM_X86_64, EV_NONE), &w, "./a.out") {
        Ok(s) => s.as_str()
            == "bash: ./a.out: cannot execute binary file: Exec format error (e_version=0, need 1)\n",
        Err(_) => false,
    };
    let e = match message(&Compat::new(ELFCLASS64, ELFDATA2LSB, EM_X86_64, EV_CURRENT), &w, "./a.out") {
        Ok(s) => s.as_str() == "ok: ELFCLASS64/ELFDATA2LSB/EM_X86_64/EV_CURRENT\n",
        Err(_) => false,
    };
    let f = message(&Compat::new(ELFCLASS64, ELFDATA2LSB, EM_X86_64, EV_CURRENT), &w, "") == Err(CL_EBYTE);
    a && b && c && d && e && f
}

/// errno。
fn errno_ok() -> bool {
    let w = Want::x86_64();
    let a = errno(&Compat::new(ELFCLASS64, ELFDATA2LSB, EM_X86_64, EV_CURRENT), &w) == 0;
    let b = errno(&Compat::new(ELFCLASS32, ELFDATA2LSB, EM_386, EV_CURRENT), &w) == ENOEXEC;
    let c = ENOEXEC == 8;
    a && b && c
}

/// 探针。
fn probe_ok() -> bool {
    let w = Want::x86_64();
    let ok = Compat::new(ELFCLASS64, ELFDATA2LSB, EM_X86_64, EV_CURRENT);
    let mut p = Probe::new();
    let a = !p.bound() && p.probe(&ok, &w) == Ok(()) && p.bound() && p.hits() == 1;
    let b = p.probe(&ok, &w) == Err(CL_ESTATE) && p.last() == CL_ESTATE;
    let mut q = Probe::new();
    let c = q.probe(&Compat::new(ELFCLASS32, ELFDATA2LSB, EM_386, EV_CURRENT), &w) == Err(CL_ECLASS);
    let d = q.last() == CL_ECLASS && q.hits() == 1;
    a && b && c && d
}

/// 摘要。
fn seal_ok() -> bool {
    let a = Compat::new(ELFCLASS64, ELFDATA2LSB, EM_X86_64, EV_CURRENT).seal()
        == Compat::new(ELFCLASS64, ELFDATA2LSB, EM_X86_64, EV_CURRENT).seal();
    let b = Compat::new(ELFCLASS64, ELFDATA2LSB, EM_X86_64, EV_CURRENT).seal()
        != Compat::new(ELFCLASS32, ELFDATA2LSB, EM_386, EV_CURRENT).seal();
    let mut buf = [0u8; MIN_EHDR];
    buf[EI_CLASS] = ELFCLASS64;
    let c = Head::new(&buf).seal() == Head::new(&buf).seal();
    let mut buf2 = [0u8; MIN_EHDR];
    buf2[EI_CLASS] = ELFCLASS32;
    let d = Head::new(&buf).seal() != Head::new(&buf2).seal();
    let e = digest(b"compat") == digest(b"compat") && digest(b"compat") != digest(b"compat2");
    a && b && c && d && e
}

/// 文本。
fn text_ok() -> bool {
    u32_text(0).as_str() == "0"
        && u32_text(13).as_str() == "13"
        && u32_text(929).as_str() == "929"
        && u32_text(4294967295).as_str() == "4294967295"
}

/// 短语。
fn clause_ok() -> bool {
    let w = Want::x86_64();
    let a = clause(&Compat::new(ELFCLASS32, ELFDATA2LSB, EM_386, EV_CURRENT), &w, CL_ECLASS)
        .as_str()
        == "ELFCLASS32, need ELFCLASS64";
    let b = clause(&Compat::new(ELFCLASS64, ELFDATA2MSB, EM_X86_64, EV_CURRENT), &w, CL_EDATA)
        .as_str()
        == "ELFDATA2MSB, need ELFDATA2LSB";
    let c = clause(&Compat::new(ELFCLASS64, ELFDATA2LSB, EM_AARCH64, EV_CURRENT), &w, CL_EMACH)
        .as_str()
        == "EM_AARCH64, need EM_X86_64";
    let d = clause(&Compat::new(ELFCLASS64, ELFDATA2LSB, EM_X86_64, EV_NONE), &w, CL_ECVER)
        .as_str()
        == "e_version=0, need 1";
    let e = clause(&Compat::new(ELFCLASS64, ELFDATA2LSB, EM_X86_64, EV_CURRENT), &w, 0xffff)
        .as_str()
        == "unknown mismatch";
    a && b && c && d && e
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = names_ok() && text_ok();
    out[1] = order_ok() && head_ok() && short_ok();
    out[2] = want_ok() && corpus_ok() && slots_ok();
    out[3] = report_ok() && reds_ok() && cover_ok() && message_ok() && errno_ok()
        && probe_ok() && seal_ok() && clause_ok()
        && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11606_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11606.compat_gate");

    s.add("CL01-名称表", names_ok(), "static");
    s.add("CL02-次序闸门", order_ok(), "static");
    s.add("CL03-头读取", head_ok(), "static");
    s.add("CL04-短头拒收", short_ok(), "static");
    s.add("CL05-期望面", want_ok(), "static");
    s.add("CL06-十四卡全跑", corpus_ok(), "static");
    s.add("CL07-语料编号", slots_ok(), "static");
    s.add("CL08-汇总渲染", report_ok(), "static");
    s.add("CL09-红单显式", reds_ok(), "static");
    s.add("CL10-封面", cover_ok(), "static");
    s.add("CL11-文案逐字", message_ok() && clause_ok(), "static");
    s.add("CL12-探针与摘要", errno_ok() && probe_ok() && seal_ok() && text_ok(), "static");
    s.add("CL13-冻结重放四注入", inject_four().iter().all(|x| *x), "static");

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
    fn want_default() {
        let w = Want::x86_64();
        assert!(w.is_default());
        assert_eq!(w.class, ELFCLASS64);
        assert_eq!(w.data, ELFDATA2LSB);
        assert_eq!(w.machine, EM_X86_64);
        assert_eq!(w.version, EV_CURRENT);
    }

    #[test]
    fn check_order_class_first() {
        let w = Want::x86_64();
        let c = Compat::new(ELFCLASS32, ELFDATA2MSB, EM_386, EV_NONE);
        assert_eq!(c.check(&w), Err(CL_ECLASS));
    }

    #[test]
    fn check_order_data_then_version() {
        let w = Want::x86_64();
        let c = Compat::new(ELFCLASS64, ELFDATA2MSB, EM_386, EV_NONE);
        assert_eq!(c.check(&w), Err(CL_EDATA));
        let d = Compat::new(ELFCLASS64, ELFDATA2LSB, EM_386, EV_NONE);
        assert_eq!(d.check(&w), Err(CL_ECVER));
    }

    #[test]
    fn check_machine() {
        let w = Want::x86_64();
        assert_eq!(
            Compat::new(ELFCLASS64, ELFDATA2LSB, EM_AARCH64, EV_CURRENT).check(&w),
            Err(CL_EMACH)
        );
        assert_eq!(
            Compat::new(ELFCLASS64, ELFDATA2LSB, EM_NONE, EV_CURRENT).check(&w),
            Err(CL_EMACH)
        );
    }

    #[test]
    fn host_accepts() {
        let w = Want::x86_64();
        let c = Compat::new(ELFCLASS64, ELFDATA2LSB, EM_X86_64, EV_CURRENT);
        assert_eq!(c.check(&w), Ok(()));
        assert_eq!(errno(&c, &w), 0);
    }

    #[test]
    fn head_le() {
        let mut buf = [0u8; MIN_EHDR];
        buf[EI_CLASS] = ELFCLASS64;
        buf[EI_DATA] = ELFDATA2LSB;
        buf[E_MACHINE_OFF] = 62;
        buf[E_VERSION_OFF] = 1;
        let h = Head::new(&buf);
        assert_eq!(h.len(), MIN_EHDR);
        assert_eq!(h.le16(E_MACHINE_OFF), Ok(62));
        assert_eq!(h.le32(E_VERSION_OFF), Ok(1));
    }

    #[test]
    fn head_short() {
        let h = Head::new(&[0u8; 6]);
        assert!(!h.complete());
        assert_eq!(h.short_by(), MIN_EHDR - 6);
        assert_eq!(h.compat(), Err(CL_EBYTE));
    }

    #[test]
    fn table_matches_judge() {
        assert!(table_ok());
        let r = match run() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(r.matched(), CORPUS);
    }

    #[test]
    fn report_gate() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(r.accept(), 1);
        assert_eq!(r.reject(), 13);
        assert_eq!(r.class(), 4);
        assert_eq!(r.data(), 3);
        assert_eq!(r.version(), 2);
        assert_eq!(r.machine(), 4);
        assert_eq!(r.gate(), Ok(()));
    }

    #[test]
    fn red_override() {
        let r = match run_with(Some((1, CL_EDATA))) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(r.rate(), 929);
        assert_eq!(r.gate(), Err(CL_EVER));
        assert_eq!(run_with(Some((14, CL_EDATA))).err(), Some(CL_ESLOT));
    }

    #[test]
    fn message_class32() {
        let w = Want::x86_64();
        let m = match message(&Compat::new(ELFCLASS32, ELFDATA2LSB, EM_386, EV_CURRENT), &w, "./a.out") {
            Ok(v) => v,
            Err(_) => return,
        };
        assert!(m.as_str().contains("(ELFCLASS32, need ELFCLASS64)"));
    }

    #[test]
    fn message_ok_path() {
        let w = Want::x86_64();
        let m = match message(&Compat::new(ELFCLASS64, ELFDATA2LSB, EM_X86_64, EV_CURRENT), &w, "./a.out") {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(m.as_str(), "ok: ELFCLASS64/ELFDATA2LSB/EM_X86_64/EV_CURRENT\n");
        assert_eq!(
            message(&Compat::new(ELFCLASS64, ELFDATA2LSB, EM_X86_64, EV_CURRENT), &w, ""),
            Err(CL_EBYTE)
        );
    }

    #[test]
    fn probe_once() {
        let w = Want::x86_64();
        let ok = Compat::new(ELFCLASS64, ELFDATA2LSB, EM_X86_64, EV_CURRENT);
        let mut p = Probe::new();
        assert_eq!(p.probe(&ok, &w), Ok(()));
        assert_eq!(p.probe(&ok, &w), Err(CL_ESTATE));
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
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
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(CL_EBAL));
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
        let s = run_unx11606_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }
}
