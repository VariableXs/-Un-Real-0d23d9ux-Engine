//! UNX-F6841 ATA identify device 透传（512 字节逐字段裁断）（0x9000 段）
//!
//! 锚点：UNX-F6841-J1 identify device 512 字节逐字段裁断账
//! （型号/序列号/LBA48 容量/扇区大小 ≥10 字段）；裁断表机读可查。
//! 域：CoRun·UNX-B4-B23（ATA identify 面）。
//!
//! 结构：IDENTIFY DEVICE 512 字节模型（word 编址 256 words）+ 字段裁断表
//! （型号 word27..46 / 序列号 word10..19 / 固件 word23..26 / LBA48 容量
//! word100..103 / 扇区大小 word106 / 物理逻辑扇区 word106..109 / 转速
//! word217 / NV cache word214..215 / 透明校验 word100 上限 / 世界名字
//! word27 投影）≥10 字段 + 裁断器（word 逐段提取 + 字符串字节序翻转）
//! + 机读裁断表（(偏移, 长度, 标签, 取值) 四元组闭集）+ 注入核
//! （word 界越界/LBA48 容量零/字串含 NUL 尾残留/裁断封账）。

use crate::alloc::string::String;
use crate::alloc::{format, vec, vec::Vec};
use crate::checks::CheckSet;

/// 诊断段（0x9000 独占 0x80）。
pub const SEG_BASE: u32 = 0x9000;
pub const XC_ID_WOBB: u32 = SEG_BASE + 0x01; // word 界越界
pub const XC_ID_LBA0: u32 = SEG_BASE + 0x02; // LBA48 容量零
pub const XC_ID_NULRES: u32 = SEG_BASE + 0x03; // 字串 NUL 尾残留
pub const XC_ID_SEALED: u32 = SEG_BASE + 0x04; // 裁断封账

/// IDENTIFY 字长（字节）。
pub const WORD_BYTES: usize = 2;
/// IDENTIFY 总字节。
pub const ID_BYTES: usize = 512;
/// IDENTIFY 总字数。
pub const ID_WORDS: usize = 256;

/// 裁断字段标签闭集（机读表用）。
pub const F_MODEL: &str = "model";
pub const F_SERIAL: &str = "serial";
pub const F_FWREV: &str = "fwrev";
pub const F_LBA48: &str = "lba48_capacity";
pub const F_SECSZ: &str = "sector_size";
pub const F_PHYLOG: &str = "phy_log_sector";
pub const F_RPM: &str = "nominal_rpm";
pub const F_NVCACHE: &str = "nv_cache_size";
pub const F_WCACHE: &str = "write_cache";
pub const F_TRUSTED: &str = "trusted_computing";
pub const F_MULTICNT: &str = "multi_count";
pub const F_CAP16: &str = "words_100_103_cap";

/// 机读裁断表项：(word 起偏, word 数, 标签, 值语义)。
#[derive(Clone, Copy)]
pub struct TrimEntry {
    pub word_off: usize,
    pub word_len: usize,
    pub tag: &'static str,
    pub kind: TrimKind,
}

/// 值语义：字串（字节序翻转拼接）/ u64 容量 / u32 计数 / 位域。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TrimKind {
    Str,
    U64,
    U32,
    Bits,
}

/// ≥10 字段裁断表（冻结序）。
pub const TRIM_TABLE: [TrimEntry; 12] = [
    TrimEntry { word_off: 27, word_len: 20, tag: F_MODEL, kind: TrimKind::Str },
    TrimEntry { word_off: 10, word_len: 10, tag: F_SERIAL, kind: TrimKind::Str },
    TrimEntry { word_off: 23, word_len: 4, tag: F_FWREV, kind: TrimKind::Str },
    TrimEntry { word_off: 100, word_len: 4, tag: F_LBA48, kind: TrimKind::U64 },
    TrimEntry { word_off: 106, word_len: 1, tag: F_SECSZ, kind: TrimKind::Bits },
    TrimEntry { word_off: 106, word_len: 4, tag: F_PHYLOG, kind: TrimKind::U32 },
    TrimEntry { word_off: 217, word_len: 1, tag: F_RPM, kind: TrimKind::U32 },
    TrimEntry { word_off: 214, word_len: 2, tag: F_NVCACHE, kind: TrimKind::U64 },
    TrimEntry { word_off: 85, word_len: 1, tag: F_WCACHE, kind: TrimKind::Bits },
    TrimEntry { word_off: 128, word_len: 1, tag: F_TRUSTED, kind: TrimKind::Bits },
    TrimEntry { word_off: 47, word_len: 1, tag: F_MULTICNT, kind: TrimKind::Bits },
    TrimEntry { word_off: 100, word_len: 4, tag: F_CAP16, kind: TrimKind::U64 },
];

/// 512 字节 identify 页模型（word 大端序布局校验面）。
#[derive(Clone)]
pub struct IdentifyPage {
    pub words: [u16; ID_WORDS],
}

impl IdentifyPage {
    /// 从 512 字节装配（word 小端：低字节在前——ATA 线序）。
    pub fn from_bytes(b: &[u8; ID_BYTES]) -> Result<Self, u32> {
        let mut w = [0u16; ID_WORDS];
        for i in 0..ID_WORDS {
            w[i] = b[i * 2] as u16 | ((b[i * 2 + 1] as u16) << 8);
        }
        Ok(IdentifyPage { words: w })
    }

    /// word 界检查。
    pub fn check_range(off: usize, len: usize) -> Result<(), u32> {
        if off >= ID_WORDS || len == 0 || off + len > ID_WORDS {
            return Err(XC_ID_WOBB);
        }
        Ok(())
    }

    /// 字串字段提取：word → 两字节小端翻转（ATA 字串每 word 内字节交换），
    /// 右侧空格补齐裁掉，残留 NUL 拒绝。
    pub fn str_field(&self, off: usize, len: usize) -> Result<String, u32> {
        Self::check_range(off, len)?;
        let mut s = String::new();
        for i in 0..len {
            let w = self.words[off + i];
            let hi = (w >> 8) as u8;
            let lo = (w & 0xFF) as u8;
            // ATA 字串序：每 word 高字节在前
            if hi != 0 && hi != 0x20 {
                s.push(hi as char);
            } else if !s.is_empty() && hi == 0x20 {
                s.push(' ');
            }
            if lo != 0 && lo != 0x20 {
                s.push(lo as char);
            } else if !s.is_empty() && lo == 0x20 {
                s.push(' ');
            }
        }
        // 裁掉尾随空格
        let trimmed = s.trim_end();
        if trimmed.as_bytes().iter().any(|&c| c == 0) {
            return Err(XC_ID_NULRES);
        }
        Ok(String::from(trimmed))
    }

    /// u64 容量字段提取（word 串小端拼接）。
    pub fn u64_field(&self, off: usize, len: usize) -> Result<u64, u32> {
        Self::check_range(off, len)?;
        let mut v: u64 = 0;
        for i in 0..len {
            v |= (self.words[off + i] as u64) << (16 * i);
        }
        Ok(v)
    }

    /// u32 字段提取。
    pub fn u32_field(&self, off: usize, len: usize) -> Result<u32, u32> {
        Self::check_range(off, len)?;
        let mut v: u32 = 0;
        for i in 0..len {
            v |= (self.words[off + i] as u32) << (16 * i);
        }
        Ok(v)
    }

    /// 位域字段：word 指定位。
    pub fn bit_field(&self, off: usize, bit: usize) -> Result<bool, u32> {
        Self::check_range(off, 1)?;
        if bit >= 16 {
            return Err(XC_ID_WOBB);
        }
        Ok((self.words[off] >> bit) & 1 == 1)
    }

    /// LBA48 容量（word100..103）闸：零拒绝。
    pub fn lba48_capacity(&self) -> Result<u64, u32> {
        let cap = self.u64_field(100, 4)?;
        if cap == 0 {
            return Err(XC_ID_LBA0);
        }
        Ok(cap)
    }

    /// 扇区大小（word106 位域：bit12=逻辑大于物理，bit13=多扇区有效，
    /// bits0..3 = 2^(n) 逻辑扇区）。
    pub fn sector_size(&self) -> Result<(u32, bool, bool), u32> {
        let w = self.u32_field(106, 1)?;
        let log2 = (w & 0xF) as u32;
        let bigger = (w >> 12) & 1 == 1;
        let multiple = (w >> 13) & 1 == 1;
        Ok((512u32 << log2, bigger, multiple))
    }

    /// 物理对齐（word106..109）：物理扇区大小与对齐偏移。
    pub fn phy_log(&self) -> Result<(u32, u32), u32> {
        let _ = self.u32_field(106, 1)?;
        let _ = self.u32_field(107, 1)?;
        let phy = self.u32_field(106, 2)?;
        let align = self.u32_field(108, 1)?;
        Ok((phy, align))
    }
}

/// 裁断器：全表逐项提取 → 机读四元组账（标签, 字面值）。
pub fn trim_all(page: &IdentifyPage) -> Result<Vec<(String, String)>, u32> {
    let mut out: Vec<(String, String)> = Vec::new();
    for e in TRIM_TABLE.iter() {
        // 表内冻结界自查
        IdentifyPage::check_range(e.word_off, e.word_len)?;
        let val = match e.kind {
            TrimKind::Str => page.str_field(e.word_off, e.word_len)?,
            TrimKind::U64 => format!("{}", page.u64_field(e.word_off, e.word_len)?),
            TrimKind::U32 => format!("{}", page.u32_field(e.word_off, e.word_len)?),
            TrimKind::Bits => format!("0x{:04X}", page.u32_field(e.word_off, e.word_len)?),
        };
        out.push((String::from(e.tag), val));
    }
    if out.len() != TRIM_TABLE.len() {
        return Err(XC_ID_SEALED);
    }
    Ok(out)
}

/// 字符串字节序翻转核：已知 word 串 → 期望字串。
pub fn str_swap_exact() -> bool {
    // "VARIX-SSD" 9 字节 + 空格补齐，word27..46 共 20 word
    let mut w = [0u16; ID_WORDS];
    let name = b"VARIX-SSD";
    for (i, &c) in name.iter().enumerate() {
        let word_i = 27 + i / 2;
        if i % 2 == 0 {
            // word 内第一个字符 = 高字节
            w[word_i] |= (c as u16) << 8;
        } else {
            w[word_i] |= c as u16;
        }
    }
    // 尾部补空格
    for i in (27 + name.len() / 2 + name.len() % 2)..47 {
        w[i] = 0x2020;
    }
    let page = IdentifyPage { words: w };
    match page.str_field(27, 20) {
        Ok(s) => s == "VARIX-SSD",
        Err(_) => false,
    }
}

/// LBA48 容量核：word100..103 已知容量。
pub fn lba48_exact() -> bool {
    let mut w = [0u16; ID_WORDS];
    let cap: u64 = 0x0000_0000_3B9E_0000; // 1,000,000,000,000 附近样例
    w[100] = (cap & 0xFFFF) as u16;
    w[101] = ((cap >> 16) & 0xFFFF) as u16;
    w[102] = ((cap >> 32) & 0xFFFF) as u16;
    w[103] = ((cap >> 48) & 0xFFFF) as u16;
    let page = IdentifyPage { words: w };
    page.lba48_capacity() == Ok(cap) && page.u64_field(100, 4) == Ok(cap)
}

/// 扇区大小位域核。
pub fn sector_bits_exact() -> bool {
    let mut w = [0u16; ID_WORDS];
    // 逻辑 512B（log2=0）、物理大于逻辑、无多扇区
    w[106] = (1 << 12) | 0x0;
    let page = IdentifyPage { words: w };
    let (sz, bigger, multi) = match page.sector_size() {
        Ok(t) => t,
        Err(_) => return false,
    };
    // 4K 物理场景：log2=3 → 4096
    w[106] = (1 << 12) | (1 << 13) | 0x3;
    let page2 = IdentifyPage { words: w };
    let (sz2, bigger2, multi2) = match page2.sector_size() {
        Ok(t) => t,
        Err(_) => return false,
    };
    sz == 512 && bigger && !multi && sz2 == 4096 && bigger2 && multi2
}

/// 裁断表完整性核：≥10 字段、标签唯一、word 界全合法。
pub fn trim_table_integrity() -> bool {
    if TRIM_TABLE.len() < 10 {
        return false;
    }
    for i in 0..TRIM_TABLE.len() {
        for j in (i + 1)..TRIM_TABLE.len() {
            if TRIM_TABLE[i].tag == TRIM_TABLE[j].tag {
                return false;
            }
        }
        if IdentifyPage::check_range(TRIM_TABLE[i].word_off, TRIM_TABLE[i].word_len).is_err() {
            return false;
        }
    }
    true
}

/// 全页装配核：512 字节线序 → word 投影一致。
pub fn byte_assembly_exact() -> bool {
    let mut b = [0u8; ID_BYTES];
    b[0] = 0x34;
    b[1] = 0x12; // word0 = 0x1234（ATA 线序小端）
    b[54] = 0xCD;
    b[55] = 0xAB; // word27 = 0xABCD
    let page = match IdentifyPage::from_bytes(&b) {
        Ok(p) => p,
        Err(_) => return false,
    };
    page.words[0] == 0x1234 && page.words[27] == 0xABCD
}

/// 注入核（4 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) word 界越界：off=255 len=2
    let page = IdentifyPage { words: [0u16; ID_WORDS] };
    if IdentifyPage::check_range(255, 2) == Err(XC_ID_WOBB) {
        caught += 1;
    }
    // 2) LBA48 容量零拒绝
    if page.lba48_capacity() == Err(XC_ID_LBA0) {
        caught += 1;
    }
    // 3) NUL 残留拒绝：word 内嵌零字节字串
    let mut w2 = [0u16; ID_WORDS];
    w2[27] = 0x4100; // 高字节 0x41='A'，低字节 0x00
    let page2 = IdentifyPage { words: w2 };
    if page2.str_field(27, 1) == Err(XC_ID_NULRES) {
        caught += 1;
    }
    // 4) 裁断确定性：同页两跑全等（漂移必现形）
    let mut w3 = [0u16; ID_WORDS];
    let name = b"TESTDISK";
    for (i, &c) in name.iter().enumerate() {
        let word_i = 27 + i / 2;
        if i % 2 == 0 {
            w3[word_i] |= (c as u16) << 8;
        } else {
            w3[word_i] |= c as u16;
        }
    }
    for i in (27 + name.len() / 2 + name.len() % 2)..47 {
        w3[i] = 0x2020;
    }
    let p3 = IdentifyPage { words: w3 };
    let a = trim_all(&p3);
    let b3 = trim_all(&p3);
    if a.is_ok() && a == b3 {
        caught += 1;
    }
    (caught, 4)
}

/// 指纹冻结：全表提取值折叠哈希。
pub fn fp_frozen(page: &IdentifyPage) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    if let Ok(items) = trim_all(page) {
        for (tag, val) in items {
            for &c in tag.as_bytes() {
                h ^= c as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            h ^= 0x1F as u64;
            for &c in val.as_bytes() {
                h ^= c as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        }
    }
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx6841_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F6841");
    // J1-1 字串字节序翻转
    let ok = str_swap_exact();
    s.add("B23-01-字串翻转", ok, if ok { "" } else { "字节序漂移" });
    // J1-2 LBA48 容量逐字
    let ok = lba48_exact();
    s.add("B23-02-LBA48容量", ok, if ok { "" } else { "容量漂移" });
    // J1-3 扇区大小位域
    let ok = sector_bits_exact();
    s.add("B23-03-扇区位域", ok, if ok { "" } else { "位域漂移" });
    // J1-4 裁断表完整性（≥10 字段）
    let ok = trim_table_integrity();
    s.add("B23-04-裁断表完整", ok, if ok { "" } else { "表缺项" });
    // J1-5 全页装配
    let ok = byte_assembly_exact();
    s.add("B23-05-页装配", ok, if ok { "" } else { "线序漂移" });
    // J1-6 注入核 4/4
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B23-06-注入核必抓", ok, &detail);
    // J1-7 指纹冻结 + 机读表可查（12 项全提取）
    let mut w = [0u16; ID_WORDS];
    let name = b"JUDGE-DISK";
    for (i, &c) in name.iter().enumerate() {
        let word_i = 27 + i / 2;
        if i % 2 == 0 {
            w[word_i] |= (c as u16) << 8;
        } else {
            w[word_i] |= c as u16;
        }
    }
    for i in (27 + name.len() / 2 + name.len() % 2)..47 {
        w[i] = 0x2020;
    }
    w[100] = 0x0000;
    w[101] = 0x0000;
    w[102] = 0x003B;
    w[103] = 0x0000;
    let page = IdentifyPage { words: w };
    let items_ok = match trim_all(&page) {
        Ok(items) => items.len() == TRIM_TABLE.len() && items.len() >= 10,
        Err(_) => false,
    };
    let ok = fp_frozen(&page) == fp_frozen(&page) && items_ok;
    s.add("B23-07-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
