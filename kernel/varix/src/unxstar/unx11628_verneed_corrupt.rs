//! UNX-F11628 .gnu.version_r 损坏判据
//!
//! 闸门次序：节存在 → 条目偏移 → 辅助链 → 条目链前进 → 计数 → 旗标 → versym → 判面。
//! 契约：纯 no_std 风格（仅 `alloc::string::String` 渲染），无 unsafe、无 unwrap/expect/panic!，失败一律 `Result<_, u32>`，域码 + 状态码恰 13 个。
//!
//! `.gnu.version_r`（`SHT_GNU_verneed`）不是一张平表，而是**两级链**：每个
//! `Elf64_Verneed`（`vn_version/vn_cnt/vn_file/vn_aux/vn_next`）挂一条
//! `Elf64_Vernaux` 链（`vna_hash/vna_flags/vna_other/vna_name/vna_next`），
//! 条目之间再由 `vn_next` 串起来。字符串全部落在 `.dynstr`，靠 `vn_file` 与
//! `vna_name` 两个偏移寻址。装载器（glibc `_dl_check_map_versions`）先按
//! `DT_VERNEEDNUM` 走条目链，再按 `vn_cnt` 走辅助链，最后拿 `.gnu.version`
//! 的 16 位索引回查。
//!
//! 四层里任何一层坏了都会在**重定位之前**拒载，而"坏"的形态并不相同：
//!
//! 1. 偏移越界：`vn_file`/`vna_name` 落在 `DT_STRSZ` 之外——直接拒载，不能
//!    拿越界偏移去读 `.dynstr`；
//! 2. 链不自洽：`vn_aux` 为 0 而 `vn_cnt>0`，或辅助链实际条数与 `vn_cnt`
//!    不等；
//! 3. 链不前进：`vn_next` 指回自身/回退，走链会死循环，必须在读第二遍前
//!    判死（不能靠"最多走 N 步"兜底，那样坏节会被静默截断）；
//! 4. 计数不符：`DT_VERNEEDNUM` 与实际解析条目数不等；
//! 5. 索引越界：`.gnu.version` 的 `vs_index` 大于"全局/本地 + 全部辅助项"
//!    的上界；
//! 6. 旗标非法：`vna_flags` 出现保留位（合法只有 `VER_FLG_BASE`、
//!    `VER_FLG_WEAK`、`VER_FLG_INFO`）。
//!
//! 本卡把"版本节偏移越界拒载"（J1）放在第 1 层，同时把其余五类同族损坏
//! 一并立成判据：坏节必须**在写任何重定位之前**被拒，且错误码落到六个域码
//! 之一。14 槽语料 8 活 6 死，活档覆盖单条、双辅助、双文件、弱版本、无版本
//! 节、verdef 共存与 versym 隐藏位。
//!
//! 本模块的拒收发生在重定位写槽之前，14 槽的期望码与账本码共用同一批常量。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfe10;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 版本需求条目容量。
pub const VNEED_MAX: usize = 4;

/// 单条目辅助项容量。
pub const AUX_MAX: usize = 3;

/// `.gnu.version` 索引容量。
pub const SYM_MAX: usize = 6;

/// 单条 Verneed 记录字节数（模型值）。
pub const VNEED_SIZE: u16 = 16;

/// 合法旗标掩码：BASE|WEAK|INFO。
pub const VER_FLAG_MASK: u16 = 0x0007;

/// `VER_FLG_BASE`。
pub const VER_FLG_BASE: u16 = 0x0001;

/// `VER_FLG_WEAK`。
pub const VER_FLG_WEAK: u16 = 0x0002;

/// `VER_FLG_INFO`。
pub const VER_FLG_INFO: u16 = 0x0004;

/// `VER_NDX_LOCAL`。
pub const VER_NDX_LOCAL: u16 = 0;

/// `VER_NDX_GLOBAL`。
pub const VER_NDX_GLOBAL: u16 = 1;

/// 隐藏位。
pub const VER_NDX_HIDDEN: u16 = 0x8000;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 版本节条目/名字偏移越过 `.dynstr`。
pub const VR_EOFF: u32 = SEG_BASE + 0x01;
/// `vn_aux`/`vn_cnt`/辅助链条数不自洽。
pub const VR_EAUX: u32 = SEG_BASE + 0x02;
/// `vn_next` 不前进（自指或回退）。
pub const VR_ELOOP: u32 = SEG_BASE + 0x03;
/// `DT_VERNEEDNUM` 与实际条目数不符。
pub const VR_ECOUNT: u32 = SEG_BASE + 0x04;
/// `.gnu.version` 索引越过版本上界。
pub const VR_EVERSYM: u32 = SEG_BASE + 0x05;
/// `vna_flags` 含保留位。
pub const VR_EFLAGS: u32 = SEG_BASE + 0x06;
/// 账本状态错。
pub const VR_ESTATE: u32 = SEG_BASE + 0x07;
/// 版本/结构期望不符。
pub const VR_EVER: u32 = SEG_BASE + 0x08;
/// 对账不平。
pub const VR_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位越界。
pub const VR_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与常量表不符。
pub const VR_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结指纹失配。
pub const VR_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无可观测样本。
pub const VR_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 语料汇总。
pub const OP_CORPUS: u8 = 1;
/// 偏移检查。
pub const OP_OFFSET: u8 = 2;
/// 链检查。
pub const OP_CHAIN: u8 = 3;
/// 计数检查。
pub const OP_COUNT: u8 = 4;
/// 旗标检查。
pub const OP_FLAGS: u8 = 5;
/// versym 检查。
pub const OP_VERSYM: u8 = 6;

// ---------------------------------------------------------------------------
// FNV-1a
// ---------------------------------------------------------------------------

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 折入一个字节。
fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

/// 折入一个字（小端逐字节）。
fn fold_word(h: u64, w: u64) -> u64 {
    let mut acc = h;
    let mut i = 0u32;
    while i < 8 {
        acc = fold_byte(acc, ((w >> (i * 8)) & 0xff) as u8);
        i += 1;
    }
    acc
}

/// 字节串指纹。
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
pub fn u32_text(v: u32) -> String {
    let mut buf = [0u8; 10];
    let mut n = 0usize;
    let mut x = v;
    if x == 0 {
        if let Some(b) = buf.get_mut(0) {
            *b = b'0';
        }
        n = 1;
    }
    while x > 0 {
        if let Some(b) = buf.get_mut(n) {
            *b = b'0' + (x % 10) as u8;
        }
        n += 1;
        x /= 10;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        if let Some(b) = buf.get(i) {
            s.push(*b as char);
        }
    }
    s
}

/// 定宽十六进制文本。
pub fn hex_text(v: u32) -> String {
    const HEX: [u8; 16] = [
        b'0', b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', b'a', b'b', b'c', b'd', b'e',
        b'f',
    ];
    let mut s = String::from("0x");
    let mut shift = 28i32;
    while shift >= 0 {
        let nib = ((v >> shift) & 0x0f) as usize;
        s.push(HEX.get(nib).copied().unwrap_or(b'0') as char);
        shift -= 4;
    }
    s
}

/// `subject: tail`。
pub fn fault_text(subject: &str, tail: &str) -> String {
    let mut s = String::new();
    s.push_str(subject);
    s.push_str(": ");
    s.push_str(tail);
    s
}

// ---------------------------------------------------------------------------
// 辅助项
// ---------------------------------------------------------------------------

/// 一条 `Elf64_Vernaux`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Vernaux {
    /// 版本名哈希（`ELF hash`）。
    pub hash: u32,
    /// 旗标。
    pub flags: u16,
    /// `.gnu.version` 使用的版本序号。
    pub other: u16,
    /// 名字。
    pub name: &'static str,
    /// 名字在 `.dynstr` 的偏移。
    pub name_off: u16,
}

impl Vernaux {
    /// 构造。
    pub const fn new(hash: u32, flags: u16, other: u16, name: &'static str, off: u16) -> Vernaux {
        Vernaux { hash, flags, other, name, name_off: off }
    }

    /// 空项。
    pub const fn blank() -> Vernaux {
        Vernaux { hash: 0, flags: 0, other: 0, name: "", name_off: 0 }
    }

    /// 旗标是否只有合法位。
    pub const fn flags_ok(&self) -> bool {
        (self.flags & !VER_FLAG_MASK) == 0
    }

    /// 是否弱版本。
    pub const fn is_weak(&self) -> bool {
        (self.flags & VER_FLG_WEAK) != 0
    }

    /// 是否基版本。
    pub const fn is_base(&self) -> bool {
        (self.flags & VER_FLG_BASE) != 0
    }

    /// 是否信息版本。
    pub const fn is_info(&self) -> bool {
        (self.flags & VER_FLG_INFO) != 0
    }

    /// 名字偏移在字符串表内。
    pub const fn name_in(&self, strsz: u16) -> bool {
        self.name_off < strsz
    }

    /// 序号是否落在可用范围（0 本地、1 全局、其余辅助项）。
    pub const fn within(&self, total_aux: u16) -> bool {
        self.other <= total_aux
    }
}

// ---------------------------------------------------------------------------
// 需求条目
// ---------------------------------------------------------------------------

/// 一条 `Elf64_Verneed` 及其辅助链。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verneed {
    /// 依赖文件名。
    pub file: &'static str,
    /// 文件名在 `.dynstr` 的偏移。
    pub file_off: u16,
    /// 结构版本（恒 1）。
    pub version: u16,
    /// 声明辅助项数。
    pub cnt: u16,
    /// 辅助链偏移。
    pub aux_off: u16,
    /// 下一条目相对节首的偏移（0 终止）。
    pub next: u16,
    /// 解析出的辅助项。
    pub aux: [Vernaux; AUX_MAX],
    /// 辅助项实际条数。
    pub aux_len: u8,
}

impl Verneed {
    /// 构造。
    pub const fn new(file: &'static str, file_off: u16) -> Verneed {
        Verneed {
            file,
            file_off,
            version: 1,
            cnt: 0,
            aux_off: VNEED_SIZE,
            next: 0,
            aux: [Vernaux::blank(); AUX_MAX],
            aux_len: 0,
        }
    }

    /// 追加辅助项并同步 `cnt`。
    pub fn add_aux(&mut self, a: Vernaux) -> bool {
        if (self.aux_len as usize) >= AUX_MAX {
            return false;
        }
        if let Some(slot) = self.aux.get_mut(self.aux_len as usize) {
            *slot = a;
        }
        self.aux_len += 1;
        self.cnt = self.aux_len as u16;
        true
    }

    /// 辅助项数。
    pub const fn aux_count(&self) -> usize {
        self.aux_len as usize
    }

    /// 取辅助项。
    pub fn aux_at(&self, i: usize) -> Option<Vernaux> {
        if i >= self.aux_len as usize {
            return None;
        }
        self.aux.get(i).copied()
    }

    /// 声明的辅助项数是否等于实际条数。
    pub const fn count_ok(&self) -> bool {
        self.cnt == self.aux_len as u16
    }

    /// 辅助链偏移是否自洽。
    pub fn aux_ok(&self) -> bool {
        if self.cnt == 0 {
            return self.aux_len == 0;
        }
        self.aux_off != 0 && self.aux_len > 0 && self.count_ok()
    }

    /// 文件名偏移在字符串表内。
    pub const fn file_in(&self, strsz: u16) -> bool {
        self.file_off < strsz
    }

    /// 结构版本合法。
    pub const fn version_ok(&self) -> bool {
        self.version == 1
    }

    /// 弱辅助项数。
    pub fn weak_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.aux_len as usize {
            if let Some(a) = self.aux.get(i) {
                if a.is_weak() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 本条目所有辅助项的旗标都合法、偏移都在表内。
    pub fn aux_sane(&self, strsz: u16) -> bool {
        let mut i = 0usize;
        while i < self.aux_len as usize {
            match self.aux.get(i) {
                Some(a) => {
                    if !a.flags_ok() || !a.name_in(strsz) {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 名字偏移是否越界（返回越界值用于文案）。
    pub fn first_bad_name_off(&self, strsz: u16) -> Option<u16> {
        if !self.file_in(strsz) {
            return Some(self.file_off);
        }
        let mut i = 0usize;
        while i < self.aux_len as usize {
            if let Some(a) = self.aux.get(i) {
                if !a.name_in(strsz) {
                    return Some(a.name_off);
                }
            }
            i += 1;
        }
        None
    }
}

// ---------------------------------------------------------------------------
// 需求节
// ---------------------------------------------------------------------------

/// `.gnu.version_r` 节模型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerSection {
    /// 条目。
    pub entries: [Verneed; VNEED_MAX],
    /// 条目数。
    pub len: u8,
    /// `.dynstr` 长度。
    pub strsz: u16,
    /// `DT_VERNEEDNUM`。
    pub verneednum: u16,
    /// 节是否存在。
    pub has_section: bool,
    /// 是否有配套 `.gnu.version_d`。
    pub has_verdef: bool,
}

impl VerSection {
    /// 空节。
    pub const fn blank() -> VerSection {
        VerSection {
            entries: [Verneed::new("", 0); VNEED_MAX],
            len: 0,
            strsz: 0,
            verneednum: 0,
            has_section: false,
            has_verdef: false,
        }
    }

    /// 构造带字符串表的节。
    pub const fn with_strsz(strsz: u16) -> VerSection {
        VerSection {
            entries: [Verneed::new("", 0); VNEED_MAX],
            len: 0,
            strsz,
            verneednum: 0,
            has_section: true,
            has_verdef: false,
        }
    }

    /// 追加条目（自动补 `verneednum` 与上一条的 `next`）。
    pub fn add(&mut self, mut v: Verneed) -> bool {
        if (self.len as usize) >= VNEED_MAX {
            return false;
        }
        v.next = 0;
        let idx = self.len as usize;
        if idx > 0 {
            if let Some(prev) = self.entries.get_mut(idx - 1) {
                prev.next = (idx as u16) * VNEED_SIZE;
            }
        }
        if let Some(slot) = self.entries.get_mut(idx) {
            *slot = v;
        }
        self.len += 1;
        self.verneednum = self.len as u16;
        true
    }

    /// 条目数。
    pub const fn len(&self) -> usize {
        self.len as usize
    }

    /// 是否空节。
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取条目。
    pub fn entry(&self, i: usize) -> Option<Verneed> {
        if i >= self.len as usize {
            return None;
        }
        self.entries.get(i).copied()
    }

    /// 全部辅助项数。
    pub fn total_aux(&self) -> u16 {
        let mut n = 0u16;
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(v) = self.entries.get(i) {
                n = n.saturating_add(v.aux_len as u16);
            }
            i += 1;
        }
        n
    }

    /// 弱辅助项数。
    pub fn weak_total(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(v) = self.entries.get(i) {
                n += v.weak_count();
            }
            i += 1;
        }
        n
    }

    /// 依赖文件名表里是否含某名字。
    pub fn needs_file(&self, file: &str) -> bool {
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(v) = self.entries.get(i) {
                if v.file == file {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 条目链偏移是否前进。
    pub fn chain_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.len as usize {
            let v = match self.entries.get(i) {
                Some(x) => *x,
                None => return false,
            };
            let expected = if i + 1 < self.len as usize {
                ((i + 1) as u16) * VNEED_SIZE
            } else {
                0
            };
            if v.next != expected {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 第一个不前进的条目序号。
    pub fn chain_break(&self) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len as usize {
            let v = match self.entries.get(i) {
                Some(x) => *x,
                None => return Some(i),
            };
            let expected = if i + 1 < self.len as usize {
                ((i + 1) as u16) * VNEED_SIZE
            } else {
                0
            };
            if v.next != expected {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// `DT_VERNEEDNUM` 与实际条目数一致。
    pub const fn count_ok(&self) -> bool {
        self.verneednum == self.len as u16
    }

    /// 所有名字偏移在表内。
    pub fn offsets_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(v) = self.entries.get(i) {
                if v.first_bad_name_off(self.strsz).is_some() {
                    return false;
                }
            }
            i += 1;
        }
        true
    }

    /// 第一个越界偏移。
    pub fn first_bad_offset(&self) -> Option<u16> {
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(v) = self.entries.get(i) {
                if let Some(off) = v.first_bad_name_off(self.strsz) {
                    return Some(off);
                }
            }
            i += 1;
        }
        None
    }

    /// 所有条目版本号合法。
    pub fn versions_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(v) = self.entries.get(i) {
                if !v.version_ok() {
                    return false;
                }
            }
            i += 1;
        }
        true
    }

    /// 所有辅助链自洽。
    pub fn aux_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(v) = self.entries.get(i) {
                if !v.aux_ok() {
                    return false;
                }
            }
            i += 1;
        }
        true
    }

    /// 第一个辅助链损坏的条目序号。
    pub fn aux_break(&self) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(v) = self.entries.get(i) {
                if !v.aux_ok() {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 旗标全部合法。
    pub fn flags_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(v) = self.entries.get(i) {
                let mut k = 0usize;
                while k < v.aux_len as usize {
                    if let Some(a) = v.aux.get(k) {
                        if !a.flags_ok() {
                            return false;
                        }
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        true
    }

    /// 第一个非法旗标值。
    pub fn first_bad_flag(&self) -> Option<u16> {
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(v) = self.entries.get(i) {
                let mut k = 0usize;
                while k < v.aux_len as usize {
                    if let Some(a) = v.aux.get(k) {
                        if !a.flags_ok() {
                            return Some(a.flags);
                        }
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        None
    }
}

// ---------------------------------------------------------------------------
// .gnu.version
// ---------------------------------------------------------------------------

/// `.gnu.version` 索引表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Versym {
    /// 16 位索引（含隐藏位）。
    pub idx: [u16; SYM_MAX],
    /// 条数。
    pub len: u8,
}

impl Versym {
    /// 空表。
    pub const fn blank() -> Versym {
        Versym { idx: [0u16; SYM_MAX], len: 0 }
    }

    /// 追加。
    pub fn push(&mut self, v: u16) -> bool {
        if (self.len as usize) >= SYM_MAX {
            return false;
        }
        if let Some(slot) = self.idx.get_mut(self.len as usize) {
            *slot = v;
        }
        self.len += 1;
        true
    }

    /// 条数。
    pub const fn len(&self) -> usize {
        self.len as usize
    }

    /// 取原始索引。
    pub fn raw(&self, i: usize) -> Option<u16> {
        if i >= self.len as usize {
            return None;
        }
        self.idx.get(i).copied()
    }

    /// 掩掉隐藏位后的版本号。
    pub fn masked(&self, i: usize) -> Option<u16> {
        match self.raw(i) {
            Some(v) => Some(v & !VER_NDX_HIDDEN),
            None => None,
        }
    }

    /// 是否隐藏位。
    pub fn hidden(&self, i: usize) -> bool {
        match self.raw(i) {
            Some(v) => (v & VER_NDX_HIDDEN) != 0,
            None => false,
        }
    }

    /// 是否本地索引。
    pub fn is_local(&self, i: usize) -> bool {
        self.masked(i) == Some(VER_NDX_LOCAL)
    }

    /// 是否全局索引。
    pub fn is_global(&self, i: usize) -> bool {
        self.masked(i) == Some(VER_NDX_GLOBAL)
    }

    /// 索引是否都在上界内。
    pub fn within(&self, total_aux: u16) -> bool {
        let mut i = 0usize;
        while i < self.len as usize {
            match self.masked(i) {
                Some(v) => {
                    if v > total_aux {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 第一个越界索引。
    pub fn first_bad(&self, total_aux: u16) -> Option<u16> {
        let mut i = 0usize;
        while i < self.len as usize {
            match self.masked(i) {
                Some(v) => {
                    if v > total_aux {
                        return Some(v);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }
}

// ---------------------------------------------------------------------------
// 镜像
// ---------------------------------------------------------------------------

/// 一份待检查的 ELF 镜像（只保留版本相关面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ElfImage {
    /// 对象名。
    pub name: &'static str,
    /// 版本需求节。
    pub ver: VerSection,
    /// 版本索引表。
    pub sym: Versym,
}

impl ElfImage {
    /// 构造。
    pub const fn new(name: &'static str, ver: VerSection, sym: Versym) -> ElfImage {
        ElfImage { name, ver, sym }
    }

    /// 版本上界（全局 + 全部辅助项）。
    pub fn bound(&self) -> u16 {
        if !self.ver.has_section {
            return VER_NDX_GLOBAL;
        }
        self.ver.total_aux().saturating_add(VER_NDX_GLOBAL)
    }
}

/// 检查阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 节存在性。
    Section,
    /// 偏移。
    Offsets,
    /// 条目链。
    Chain,
    /// 计数。
    Count,
    /// 辅助链。
    Aux,
    /// 旗标。
    Flags,
    /// versym。
    Versym,
    /// 通过。
    Done,
}

impl Stage {
    /// 文本名。
    pub const fn label(self) -> &'static str {
        match self {
            Stage::Section => "section",
            Stage::Offsets => "offsets",
            Stage::Chain => "chain",
            Stage::Count => "count",
            Stage::Aux => "aux",
            Stage::Flags => "flags",
            Stage::Versym => "versym",
            Stage::Done => "done",
        }
    }
}

/// 通过后的视图。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VersionView {
    /// 条目数。
    pub files: u8,
    /// 辅助项总数。
    pub total_aux: u16,
    /// 弱版本数。
    pub weak: u16,
    /// 有 verdef。
    pub has_verdef: bool,
    /// 有效符号数。
    pub symbols: u8,
}

impl VersionView {
    /// 空视图。
    pub const fn blank() -> VersionView {
        VersionView { files: 0, total_aux: 0, weak: 0, has_verdef: false, symbols: 0 }
    }

    /// 是否有版本需求。
    pub const fn versioned(&self) -> bool {
        self.total_aux > 0
    }
}

/// 检查轨迹。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trace {
    stages: [Stage; 8],
    ok: [bool; 8],
    len: u8,
}

impl Trace {
    /// 空轨迹。
    pub const fn blank() -> Trace {
        Trace { stages: [Stage::Section; 8], ok: [false; 8], len: 0 }
    }

    /// 记录。
    pub fn note(&mut self, st: Stage, passed: bool) {
        if (self.len as usize) < 8 {
            if let Some(slot) = self.stages.get_mut(self.len as usize) {
                *slot = st;
            }
            if let Some(slot) = self.ok.get_mut(self.len as usize) {
                *slot = passed;
            }
            self.len += 1;
        }
    }

    /// 步数。
    pub const fn len(&self) -> usize {
        self.len as usize
    }

    /// 取步。
    pub fn at(&self, i: usize) -> Option<(Stage, bool)> {
        if i >= self.len as usize {
            return None;
        }
        match (self.stages.get(i), self.ok.get(i)) {
            (Some(a), Some(b)) => Some((*a, *b)),
            _ => None,
        }
    }

    /// 最后一步。
    pub fn last(&self) -> Option<(Stage, bool)> {
        if self.len == 0 {
            return None;
        }
        self.at(self.len as usize - 1)
    }
}

/// 一次检查的判面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Judge {
    /// 结果。
    pub result: Result<VersionView, u32>,
    /// 轨迹。
    pub trace: Trace,
}

impl Judge {
    /// 错误码（成功为 0）。
    pub fn code(&self) -> u32 {
        match self.result {
            Ok(_) => 0,
            Err(c) => c,
        }
    }

    /// 是否通过。
    pub const fn passed(&self) -> bool {
        match self.result {
            Ok(_) => true,
            Err(_) => false,
        }
    }

    /// 视图。
    pub fn view(&self) -> Option<VersionView> {
        match self.result {
            Ok(v) => Some(v),
            Err(_) => None,
        }
    }

    /// 逐字文案。
    pub fn text(&self, img: &ElfImage) -> String {
        match self.result {
            Ok(v) => {
                let mut s = String::from("version-ok ");
                s.push_str(img.name);
                s.push_str(" files=");
                s.push_str(u32_text(v.files as u32).as_str());
                s.push_str(" aux=");
                s.push_str(u32_text(v.total_aux as u32).as_str());
                s.push_str(" weak=");
                s.push_str(u32_text(v.weak as u32).as_str());
                s
            }
            Err(VR_EOFF) => {
                let off = img.ver.first_bad_offset().unwrap_or(0);
                let mut s = String::from(img.name);
                s.push_str(": version need offset ");
                s.push_str(u32_text(off as u32).as_str());
                s.push_str(" beyond .dynstr size ");
                s.push_str(u32_text(img.ver.strsz as u32).as_str());
                s
            }
            Err(VR_EAUX) => {
                let idx = img.ver.aux_break().unwrap_or(0);
                let mut s = String::from(img.name);
                s.push_str(": verneed aux chain mismatch at entry ");
                s.push_str(u32_text(idx as u32).as_str());
                s
            }
            Err(VR_ELOOP) => {
                let idx = img.ver.chain_break().unwrap_or(0);
                let v = img.ver.entry(idx);
                let next = match v {
                    Some(x) => x.next,
                    None => 0,
                };
                let mut s = String::from(img.name);
                s.push_str(": verneed chain does not advance next=");
                s.push_str(hex_text(next as u32).as_str());
                s
            }
            Err(VR_ECOUNT) => {
                let mut s = String::from(img.name);
                s.push_str(": DT_VERNEEDNUM ");
                s.push_str(u32_text(img.ver.verneednum as u32).as_str());
                s.push_str(" but ");
                s.push_str(u32_text(img.ver.len as u32).as_str());
                s.push_str(" entries parsed");
                s
            }
            Err(VR_EVERSYM) => {
                let bad = img.sym.first_bad(img.bound()).unwrap_or(0);
                let mut s = String::from(img.name);
                s.push_str(": versym index ");
                s.push_str(u32_text(bad as u32).as_str());
                s.push_str(" beyond version count ");
                s.push_str(u32_text(img.bound() as u32).as_str());
                s
            }
            Err(VR_EFLAGS) => {
                let f = img.ver.first_bad_flag().unwrap_or(0);
                let mut s = String::from(img.name);
                s.push_str(": reserved version flag ");
                s.push_str(hex_text(f as u32).as_str());
                s
            }
            Err(_) => fault_text(img.name, "version need rejected"),
        }
    }
}

/// 检查一份镜像。
pub fn examine(img: &ElfImage) -> Judge {
    let mut tr = Trace::blank();

    // 0. 节缺席：没有版本需求，任何 versym 只允许 0/1。
    if !img.ver.has_section {
        tr.note(Stage::Section, true);
        if img.sym.within(VER_NDX_GLOBAL) {
            tr.note(Stage::Versym, true);
            tr.note(Stage::Done, true);
            return Judge {
                result: Ok(VersionView {
                    files: 0,
                    total_aux: 0,
                    weak: 0,
                    has_verdef: img.ver.has_verdef,
                    symbols: img.sym.len,
                }),
                trace: tr,
            };
        }
        tr.note(Stage::Versym, false);
        return Judge { result: Err(VR_EVERSYM), trace: tr };
    }
    tr.note(Stage::Section, true);

    // 1. 偏移：条目文件名与全部辅助项名都必须落表内。
    if !img.ver.offsets_ok() {
        tr.note(Stage::Offsets, false);
        return Judge { result: Err(VR_EOFF), trace: tr };
    }
    tr.note(Stage::Offsets, true);

    // 2. 辅助链自洽（cnt 与 aux_len、aux_off）。
    if !img.ver.aux_ok() {
        tr.note(Stage::Aux, false);
        return Judge { result: Err(VR_EAUX), trace: tr };
    }
    tr.note(Stage::Aux, true);

    // 3. 条目链必须前进。
    if !img.ver.chain_ok() {
        tr.note(Stage::Chain, false);
        return Judge { result: Err(VR_ELOOP), trace: tr };
    }
    tr.note(Stage::Chain, true);

    // 4. 计数。
    if !img.ver.count_ok() {
        tr.note(Stage::Count, false);
        return Judge { result: Err(VR_ECOUNT), trace: tr };
    }
    tr.note(Stage::Count, true);

    // 5. 旗标保留位。
    if !img.ver.flags_ok() {
        tr.note(Stage::Flags, false);
        return Judge { result: Err(VR_EFLAGS), trace: tr };
    }
    tr.note(Stage::Flags, true);

    // 6. versym 索引上界。
    if !img.sym.within(img.bound()) {
        tr.note(Stage::Versym, false);
        return Judge { result: Err(VR_EVERSYM), trace: tr };
    }
    tr.note(Stage::Versym, true);
    tr.note(Stage::Done, true);

    Judge {
        result: Ok(VersionView {
            files: img.ver.len,
            total_aux: img.ver.total_aux(),
            weak: img.ver.weak_total() as u16,
            has_verdef: img.ver.has_verdef,
            symbols: img.sym.len,
        }),
        trace: tr,
    }
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 场景标签。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    /// 单条目单辅助项。
    SingleAux,
    /// 单条目双辅助项。
    TwoAux,
    /// 双条目链。
    TwoFiles,
    /// 弱版本。
    WeakFlag,
    /// 无版本节。
    NoSection,
    /// 本地/全局索引。
    GlobalLocal,
    /// verdef 共存。
    WithVerdef,
    /// 隐藏位。
    HiddenBit,
    /// 文件名偏移越界。
    FileOffBad,
    /// 辅助链条数不符。
    AuxShort,
    /// 条目链回退。
    ChainBack,
    /// verneednum 不符。
    CountBad,
    /// versym 越界。
    VersymBad,
    /// 旗标保留位。
    FlagBad,
}

impl Scenario {
    /// 标签。
    pub const fn label(self) -> &'static str {
        match self {
            Scenario::SingleAux => "single-aux",
            Scenario::TwoAux => "two-aux",
            Scenario::TwoFiles => "two-files",
            Scenario::WeakFlag => "weak-flag",
            Scenario::NoSection => "no-section",
            Scenario::GlobalLocal => "global-local",
            Scenario::WithVerdef => "with-verdef",
            Scenario::HiddenBit => "hidden-bit",
            Scenario::FileOffBad => "file-off-bad",
            Scenario::AuxShort => "aux-short",
            Scenario::ChainBack => "chain-back",
            Scenario::CountBad => "count-bad",
            Scenario::VersymBad => "versym-bad",
            Scenario::FlagBad => "flag-bad",
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
    /// 期望错误码（0 表示通过）。
    pub expect: u32,
    /// 说明。
    pub note: &'static str,
}

impl Case {
    /// 是否期望通过。
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
        name: "one-aux",
        scenario: Scenario::SingleAux,
        expect: 0,
        note: "GLIBC_2.34 单条目单辅助项，偏移全部在表内",
    },
    Case {
        name: "two-aux",
        scenario: Scenario::TwoAux,
        expect: 0,
        note: "同一条目挂 GLIBC_2.2.5 与 GLIBC_2.34 两个辅助项",
    },
    Case {
        name: "two-files",
        scenario: Scenario::TwoFiles,
        expect: 0,
        note: "libc.so.6 与 libm.so.6 两条 verneed 顺序成链",
    },
    Case {
        name: "weak",
        scenario: Scenario::WeakFlag,
        expect: 0,
        note: "vna_flags=WEAK，缺版本时可降级不拒",
    },
    Case {
        name: "none",
        scenario: Scenario::NoSection,
        expect: 0,
        note: "无 .gnu.version_r 的老对象，versionsym 仅 0/1",
    },
    Case {
        name: "gl",
        scenario: Scenario::GlobalLocal,
        expect: 0,
        note: "versym 含 LOCAL=0 与 GLOBAL=1 两个特殊索引",
    },
    Case {
        name: "verdef",
        scenario: Scenario::WithVerdef,
        expect: 0,
        note: "verneed 与 verdef 并存，本卡只校需求节",
    },
    Case {
        name: "hidden",
        scenario: Scenario::HiddenBit,
        expect: 0,
        note: "versym 带 0x8000 隐藏位，掩码后落上界内",
    },
    Case {
        name: "off-bad",
        scenario: Scenario::FileOffBad,
        expect: VR_EOFF,
        note: "vn_file=200 越过 .dynstr 长度 128",
    },
    Case {
        name: "aux-short",
        scenario: Scenario::AuxShort,
        expect: VR_EAUX,
        note: "vn_cnt=2 但辅助链只解析出 1 项",
    },
    Case {
        name: "chain-back",
        scenario: Scenario::ChainBack,
        expect: VR_ELOOP,
        note: "第二条 vn_next=0x0008 回退，链不前进",
    },
    Case {
        name: "count-bad",
        scenario: Scenario::CountBad,
        expect: VR_ECOUNT,
        note: "DT_VERNEEDNUM=3 但节里只有 2 条",
    },
    Case {
        name: "vs-bad",
        scenario: Scenario::VersymBad,
        expect: VR_EVERSYM,
        note: "versym 索引 5 越过上界 3",
    },
    Case {
        name: "flag-bad",
        scenario: Scenario::FlagBad,
        expect: VR_EFLAGS,
        note: "vna_flags=0x0008 是保留位",
    },
];

/// 取槽。
pub fn slot_at(i: usize) -> Option<Case> {
    if i >= CORPUS {
        return None;
    }
    CASES.get(i).copied()
}

/// 按场景造镜像。
pub fn image_for(sc: Scenario) -> ElfImage {
    let mut ver = VerSection::with_strsz(128);
    let mut sym = Versym::blank();
    match sc {
        Scenario::SingleAux => {
            let mut v = Verneed::new("libc.so.6", 1);
            let _ = v.add_aux(Vernaux::new(0x0d69_6d69, 0, 2, "GLIBC_2.34", 13));
            let _ = sym.push(2);
            let _ = ver.add(v);
        }
        Scenario::TwoAux => {
            let mut v = Verneed::new("libc.so.6", 1);
            let _ = v.add_aux(Vernaux::new(0x0963_cf4a, 0, 2, "GLIBC_2.2.5", 13));
            let _ = v.add_aux(Vernaux::new(0x0d69_6d69, 0, 3, "GLIBC_2.34", 24));
            let _ = sym.push(2);
            let _ = sym.push(3);
            let _ = ver.add(v);
        }
        Scenario::TwoFiles => {
            let mut a = Verneed::new("libc.so.6", 1);
            let _ = a.add_aux(Vernaux::new(0x0d69_6d69, 0, 2, "GLIBC_2.34", 13));
            let mut b = Verneed::new("libm.so.6", 40);
            let _ = b.add_aux(Vernaux::new(0x0963_cf4a, 0, 3, "GLIBC_2.2.5", 24));
            let _ = sym.push(2);
            let _ = sym.push(3);
            let _ = ver.add(a);
            let _ = ver.add(b);
        }
        Scenario::WeakFlag => {
            let mut v = Verneed::new("libdl.so.2", 1);
            let _ = v.add_aux(Vernaux::new(0x0abc_1234, VER_FLG_WEAK, 2, "GLIBC_2.34", 13));
            let _ = sym.push(2);
            let _ = ver.add(v);
        }
        Scenario::NoSection => {
            ver.has_section = false;
            ver.strsz = 0;
            ver.verneednum = 0;
            let _ = sym.push(VER_NDX_LOCAL);
            let _ = sym.push(VER_NDX_GLOBAL);
        }
        Scenario::GlobalLocal => {
            let mut v = Verneed::new("libc.so.6", 1);
            let _ = v.add_aux(Vernaux::new(0x0d69_6d69, 0, 2, "GLIBC_2.34", 13));
            let _ = ver.add(v);
            let _ = sym.push(VER_NDX_LOCAL);
            let _ = sym.push(VER_NDX_GLOBAL);
            let _ = sym.push(2);
        }
        Scenario::WithVerdef => {
            ver.has_verdef = true;
            let mut v = Verneed::new("libpthread.so.0", 1);
            let _ = v.add_aux(Vernaux::new(0x0d69_6d69, 0, 2, "GLIBC_2.34", 13));
            let _ = ver.add(v);
            let _ = sym.push(2);
        }
        Scenario::HiddenBit => {
            let mut v = Verneed::new("libc.so.6", 1);
            let _ = v.add_aux(Vernaux::new(0x0d69_6d69, 0, 2, "GLIBC_2.34", 13));
            let _ = ver.add(v);
            let _ = sym.push(2 | VER_NDX_HIDDEN);
        }
        Scenario::FileOffBad => {
            let mut v = Verneed::new("libc.so.6", 200);
            let _ = v.add_aux(Vernaux::new(0x0d69_6d69, 0, 2, "GLIBC_2.34", 13));
            let _ = ver.add(v);
            let _ = sym.push(2);
        }
        Scenario::AuxShort => {
            let mut v = Verneed::new("libc.so.6", 1);
            let _ = v.add_aux(Vernaux::new(0x0d69_6d69, 0, 2, "GLIBC_2.34", 13));
            v.cnt = 2;
            let _ = ver.add(v);
            let _ = sym.push(2);
        }
        Scenario::ChainBack => {
            let mut a = Verneed::new("libc.so.6", 1);
            let _ = a.add_aux(Vernaux::new(0x0d69_6d69, 0, 2, "GLIBC_2.34", 13));
            let mut b = Verneed::new("libm.so.6", 40);
            let _ = b.add_aux(Vernaux::new(0x0963_cf4a, 0, 3, "GLIBC_2.2.5", 24));
            let _ = ver.add(a);
            let _ = ver.add(b);
            if let Some(v) = ver.entries.get_mut(1) {
                v.next = 8;
            }
            let _ = sym.push(2);
        }
        Scenario::CountBad => {
            let mut a = Verneed::new("libc.so.6", 1);
            let _ = a.add_aux(Vernaux::new(0x0d69_6d69, 0, 2, "GLIBC_2.34", 13));
            let mut b = Verneed::new("libm.so.6", 40);
            let _ = b.add_aux(Vernaux::new(0x0963_cf4a, 0, 3, "GLIBC_2.2.5", 24));
            let _ = ver.add(a);
            let _ = ver.add(b);
            ver.verneednum = 3;
            let _ = sym.push(2);
        }
        Scenario::VersymBad => {
            let mut v = Verneed::new("libc.so.6", 1);
            let _ = v.add_aux(Vernaux::new(0x0963_cf4a, 0, 2, "GLIBC_2.2.5", 13));
            let _ = v.add_aux(Vernaux::new(0x0d69_6d69, 0, 3, "GLIBC_2.34", 24));
            let _ = ver.add(v);
            let _ = sym.push(5);
        }
        Scenario::FlagBad => {
            let mut v = Verneed::new("libc.so.6", 1);
            let _ = v.add_aux(Vernaux::new(0x0d69_6d69, 0x0008, 2, "GLIBC_2.34", 13));
            let _ = ver.add(v);
            let _ = sym.push(2);
        }
    }
    ElfImage::new("./obj", ver, sym)
}

/// 求解。
pub fn solve(c: &Case) -> Judge {
    examine(&image_for(c.scenario))
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

/// 说明与期望自洽。
pub fn notes_ok() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        let c = match CASES.get(i) {
            Some(v) => v,
            None => return false,
        };
        if c.name.is_empty() || c.note.is_empty() {
            return false;
        }
        if c.accept() && c.expect != 0 {
            return false;
        }
        if !c.accept() && c.expect < SEG_BASE + 1 {
            return false;
        }
        i += 1;
    }
    true
}

/// 逐槽判面。
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
        if c.accept() && !j.passed() {
            return false;
        }
        if !c.accept() && j.passed() {
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
    offset: usize,
    chain: usize,
    count: usize,
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
            offset: 0,
            chain: 0,
            count: 0,
            flags: 0,
            matched: 0,
            red: false,
        }
    }

    /// 总数。
    pub const fn total(&self) -> usize {
        self.total
    }

    /// 接受数。
    pub const fn accept(&self) -> usize {
        self.accept
    }

    /// 拒收数。
    pub const fn reject(&self) -> usize {
        self.reject
    }

    /// 偏移桶。
    pub const fn offset(&self) -> usize {
        self.offset
    }

    /// 链桶。
    pub const fn chain(&self) -> usize {
        self.chain
    }

    /// 计数桶。
    pub const fn count(&self) -> usize {
        self.count
    }

    /// 旗标桶。
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

    /// 千分率（四舍五入）。
    pub fn rate(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        ((self.matched as u32) * 1000 + (self.total as u32) / 2) / (self.total as u32)
    }

    /// 闸门。
    pub fn gate(&self) -> Result<(), u32> {
        if self.total == 0 {
            return Err(VR_ENOBS);
        }
        if self.total != CORPUS {
            return Err(VR_EVALUE);
        }
        if self.accept != 8 || self.reject != 6 {
            return Err(VR_EBAL);
        }
        if self.offset + self.chain + self.count + self.flags != self.reject {
            return Err(VR_EBAL);
        }
        if self.red {
            return Err(VR_ESTATE);
        }
        if self.matched != self.total {
            return Err(VR_EVER);
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
        let mut s = String::from("verneed corpus=");
        s.push_str(u32_text(self.total as u32).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept as u32).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject as u32).as_str());
        s.push_str(" offset=");
        s.push_str(u32_text(self.offset as u32).as_str());
        s.push_str(" chain=");
        s.push_str(u32_text(self.chain as u32).as_str());
        s.push_str(" count=");
        s.push_str(u32_text(self.count as u32).as_str());
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

/// 跑并注入红档。
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
            None => return Err(VR_EVALUE),
        };
        let j = solve(&c);
        r.total += 1;
        if j.passed() {
            r.accept += 1;
        } else {
            r.reject += 1;
            match j.code() {
                VR_EOFF => r.offset += 1,
                VR_EAUX | VR_ELOOP => r.chain += 1,
                VR_ECOUNT | VR_EVERSYM => r.count += 1,
                VR_EFLAGS => r.flags += 1,
                _ => return Err(VR_EVER),
            }
        }
        if j.code() == c.expect {
            r.matched += 1;
        }
        i += 1;
    }
    if let Some((slot, code)) = red {
        if slot >= r.total {
            return Err(VR_ESLOT);
        }
        if code == 0 {
            return Err(VR_EVALUE);
        }
        r.matched = r.matched.saturating_sub(1);
        r.red = true;
    }
    Ok(r)
}

/// 前缀封面。
pub fn cover(n: usize) -> Result<(), u32> {
    if n > CORPUS {
        return Err(VR_ESLOT);
    }
    let mut i = 0usize;
    while i < n {
        let c = match slot_at(i) {
            Some(v) => v,
            None => return Err(VR_EVALUE),
        };
        if solve(&c).code() != c.expect {
            return Err(VR_EVALUE);
        }
        i += 1;
    }
    Ok(())
}

/// 槽码。
fn case_code(i: usize) -> u32 {
    match slot_at(i) {
        Some(c) => solve(&c).code(),
        None => VR_EVALUE,
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
            return Err(VR_ESEAL);
        }
        if (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(VR_ESLOT);
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
            return Err(VR_EBAL);
        }
        const VALS: [u32; 8] = [14, 8, 6, 1, 2, 2, 1, 929];
        const CODES: [u32; 6] = [VR_EOFF, VR_EAUX, VR_ELOOP, VR_ECOUNT, VR_EVERSYM, VR_EFLAGS];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(VR_EBAL),
            };
            if s.slot as usize != i {
                return Err(VR_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(VR_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(VR_EVALUE);
                        }
                    }
                    None => return Err(VR_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(VR_EVALUE);
                        }
                    }
                    None => return Err(VR_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(VR_EBAL);
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
            return Err(VR_ESTATE);
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
            Err(VR_ESEAL)
        }
    }

    /// 指纹。
    pub const fn root(&self) -> u64 {
        self.root
    }

    /// 冻结。
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
    let rr = run_with(Some((0, VR_EOFF)));
    let _ = l.register(count_cell(0, OP_CORPUS, r.map(|x| x.total())));
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.accept())));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.reject())));
    let _ = l.register(count_cell(3, OP_OFFSET, r.map(|x| x.offset())));
    let _ = l.register(count_cell(4, OP_CHAIN, r.map(|x| x.chain())));
    let _ = l.register(count_cell(5, OP_COUNT, r.map(|x| x.count())));
    let _ = l.register(count_cell(6, OP_FLAGS, r.map(|x| x.flags())));
    let _ = l.register(count_cell(7, OP_VERSYM, rr.map(|x| x.rate() as usize)));
    let _ = l.register(err_cell(8, OP_OFFSET, case_code(8)));
    let _ = l.register(err_cell(9, OP_CHAIN, case_code(9)));
    let _ = l.register(err_cell(10, OP_CHAIN, case_code(10)));
    let _ = l.register(err_cell(11, OP_COUNT, case_code(11)));
    let _ = l.register(err_cell(12, OP_VERSYM, case_code(12)));
    let _ = l.register(err_cell(13, OP_FLAGS, case_code(13)));
    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 辅助项与条目结构。
fn aux_struct_ok() -> bool {
    let a = Vernaux::new(0x1234, 0, 2, "GLIBC_2.34", 13);
    let b = a.flags_ok() && !a.is_weak() && !a.is_base() && !a.is_info();
    let w = Vernaux::new(0, VER_FLG_WEAK, 3, "GLIBC_2.2.5", 24);
    let c = w.is_weak() && w.flags_ok() && !w.is_base();
    let base = Vernaux::new(0, VER_FLG_BASE, 1, "BASE", 1);
    let info = Vernaux::new(0, VER_FLG_INFO, 4, "INFO", 6);
    let d = base.is_base() && info.is_info() && info.flags_ok();
    let bad = Vernaux::new(0, 0x0008, 2, "X", 13);
    let e = !bad.flags_ok() && bad.name_in(128) && !bad.name_in(4);
    let mut v = Verneed::new("libc.so.6", 1);
    let f = v.version_ok() && v.count_ok() && v.aux_ok() && v.aux_count() == 0;
    let g = v.add_aux(a) && v.add_aux(w) && v.cnt == 2 && v.aux_count() == 2;
    let h = v.add_aux(a) && !v.add_aux(a) && v.aux_count() == 3;
    b && c && d && e && f && g && h
}

/// 节结构与文件名。
fn section_ok() -> bool {
    let mut s = VerSection::with_strsz(128);
    let a = s.has_section && s.len() == 0 && s.verneednum == 0 && s.total_aux() == 0;
    let mut v = Verneed::new("libc.so.6", 1);
    let _ = v.add_aux(Vernaux::new(0x0d69_6d69, 0, 2, "GLIBC_2.34", 13));
    let b = s.add(v) && s.len() == 1 && s.verneednum == 1;
    let c = s.entry(0).map(|x| x.file) == Some("libc.so.6");
    let d = s.needs_file("libc.so.6") && !s.needs_file("libm.so.6");
    let e = s.total_aux() == 1 && s.weak_total() == 0 && s.count_ok();
    let f = s.entry(1).is_none() && !s.is_empty();
    a && b && c && d && e && f
}

/// 偏移判据。
fn offset_ok() -> bool {
    let good = image_for(Scenario::SingleAux);
    let a = good.ver.offsets_ok() && good.ver.first_bad_offset().is_none();
    let bad = image_for(Scenario::FileOffBad);
    let b = !bad.ver.offsets_ok() && bad.ver.first_bad_offset() == Some(200);
    let j = examine(&bad);
    let c = j.code() == VR_EOFF;
    let d = j
        .text(&bad)
        .as_str()
        .eq("./obj: version need offset 200 beyond .dynstr size 128");
    let e = good.ver.entry(0).map(|v| v.file_in(128)) == Some(true);
    let f = bad.ver.entry(0).map(|v| v.file_in(128)) == Some(false);
    a && b && c && d && e && f
}

/// 辅助链判据。
fn aux_ok() -> bool {
    let good = image_for(Scenario::TwoAux);
    let a = good.ver.aux_ok() && good.ver.entry(0).map(|v| v.aux_count()) == Some(2);
    let b = good.ver.entry(0).map(|v| v.aux_ok() && v.count_ok()) == Some(true);
    let bad = image_for(Scenario::AuxShort);
    let c = !bad.ver.aux_ok() && bad.ver.aux_break() == Some(0);
    let j = examine(&bad);
    let d = j.code() == VR_EAUX;
    let e = j.text(&bad).contains("verneed aux chain mismatch at entry 0");
    let f = bad.ver.entry(0).map(|v| v.cnt == 2 && v.aux_len == 1) == Some(true);
    a && b && c && d && e && f
}

/// 条目链判据。
fn chain_ok() -> bool {
    let two = image_for(Scenario::TwoFiles);
    let a = two.ver.chain_ok() && two.ver.chain_break().is_none();
    let b = two.ver.entry(0).map(|v| v.next) == Some(VNEED_SIZE);
    let c = two.ver.entry(1).map(|v| v.next) == Some(0);
    let bad = image_for(Scenario::ChainBack);
    let d = !bad.ver.chain_ok() && bad.ver.chain_break() == Some(1);
    let j = examine(&bad);
    let e = j.code() == VR_ELOOP && j.text(&bad).contains("does not advance next=0x00000008");
    let f = bad.ver.entry(1).map(|v| v.next) == Some(8);
    a && b && c && d && e && f
}

/// 计数判据。
fn count_ok() -> bool {
    let good = image_for(Scenario::TwoFiles);
    let a = good.ver.count_ok() && good.ver.verneednum == 2 && good.ver.len == 2;
    let bad = image_for(Scenario::CountBad);
    let b = !bad.ver.count_ok() && bad.ver.verneednum == 3 && bad.ver.len == 2;
    let j = examine(&bad);
    let c = j.code() == VR_ECOUNT;
    let d = j
        .text(&bad)
        .as_str()
        .eq("./obj: DT_VERNEEDNUM 3 but 2 entries parsed");
    let e = bad.ver.entry(1).is_some() && bad.ver.entry(2).is_none();
    a && b && c && d && e
}

/// 旗标与弱版本。
fn flags_ok() -> bool {
    let weak = image_for(Scenario::WeakFlag);
    let a = weak.ver.flags_ok() && weak.ver.weak_total() == 1;
    let b = weak
        .ver
        .entry(0)
        .and_then(|v| v.aux_at(0))
        .map(|x| x.is_weak())
        == Some(true);
    let bad = image_for(Scenario::FlagBad);
    let c = !bad.ver.flags_ok() && bad.ver.first_bad_flag() == Some(0x0008);
    let j = examine(&bad);
    let d = j.code() == VR_EFLAGS;
    let e = j.text(&bad).contains("reserved version flag 0x00000008");
    let f = VER_FLAG_MASK == 7 && VER_FLG_BASE | VER_FLG_WEAK | VER_FLG_INFO == 7;
    a && b && c && d && e && f
}

/// versym 判据。
fn versym_ok() -> bool {
    let good = image_for(Scenario::HiddenBit);
    let a = good.sym.len() == 1 && good.sym.hidden(0) && good.sym.masked(0) == Some(2);
    let b = good.sym.within(good.bound()) && good.bound() == 2;
    let gl = image_for(Scenario::GlobalLocal);
    let c = gl.sym.is_local(0) && gl.sym.is_global(1) && gl.sym.masked(2) == Some(2);
    let bad = image_for(Scenario::VersymBad);
    let d = !bad.sym.within(bad.bound()) && bad.sym.first_bad(bad.bound()) == Some(5);
    let j = examine(&bad);
    let e = j.code() == VR_EVERSYM;
    let f = j
        .text(&bad)
        .as_str()
        .eq("./obj: versym index 5 beyond version count 3");
    let g = bad.sym.raw(0) == Some(5) && bad.sym.masked(0) == Some(5);
    a && b && c && d && e && f && g
}

/// 无版本节。
fn no_section_ok() -> bool {
    let img = image_for(Scenario::NoSection);
    let a = !img.ver.has_section && img.ver.len() == 0;
    let j = examine(&img);
    let b = j.passed() && j.code() == 0;
    let v = match j.view() {
        Some(v) => v,
        None => return false,
    };
    let c = v.files == 0 && v.total_aux == 0 && !v.versioned();
    let d = img.bound() == VER_NDX_GLOBAL;
    let mut bad = img;
    let _ = bad.sym.push(5);
    let e = examine(&bad).code() == VR_EVERSYM;
    a && b && c && d && e
}

/// verdef 共存。
fn verdef_ok() -> bool {
    let img = image_for(Scenario::WithVerdef);
    let j = examine(&img);
    let a = j.passed() && img.ver.has_verdef;
    let v = match j.view() {
        Some(v) => v,
        None => return false,
    };
    let b = v.has_verdef && v.files == 1 && v.total_aux == 1;
    let c = img.ver.entry(0).map(|x| x.file) == Some("libpthread.so.0");
    let d = img.ver.total_aux() == 1 && img.bound() == 2;
    a && b && c && d
}

/// 阶段轨迹。
fn trace_ok() -> bool {
    let good = examine(&image_for(Scenario::SingleAux));
    let a = good.trace.len() == 8;
    let b = good.trace.at(0) == Some((Stage::Section, true));
    let c = good.trace.last() == Some((Stage::Done, true));
    let bad = examine(&image_for(Scenario::FileOffBad));
    let d = bad.trace.last() == Some((Stage::Offsets, false));
    let e = bad.trace.len() == 2;
    let f = Stage::Versym.label() == "versym" && Stage::Done.label() == "done";
    let g = good.trace.at(99).is_none();
    a && b && c && d && e && f && g
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

/// 槽与表。
fn slots_ok() -> bool {
    unique()
        && notes_ok()
        && table_ok()
        && cover(0).is_ok()
        && cover(14).is_ok()
        && cover(15) == Err(VR_ESLOT)
        && slot_at(14).is_none()
        && slot_at(0).map(|c| c.scenario) == Some(Scenario::SingleAux)
        && slot_at(13).map(|c| c.scenario) == Some(Scenario::FlagBad)
}

/// 渲染。
fn report_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.render()
        == "verneed corpus=14 accept=8 reject=6 offset=1 chain=2 count=2 flags=1 match=14 rate=1000ppm\n";
    let b = r.rate() == 1000 && r.matched() == 14;
    let c = r.green() && r.offset() + r.chain() + r.count() + r.flags() == 6;
    a && b && c
}

/// 红档。
fn reds_ok() -> bool {
    let red = match run_with(Some((4, VR_EOFF))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = red.matched() == 13 && red.rate() == 929;
    let b = !red.green() && red.gate() == Err(VR_ESTATE);
    let c = run_with(Some((14, VR_EOFF))) == Err(VR_ESLOT);
    let d = run_with(Some((0, 0))) == Err(VR_EVALUE);
    a && b && c && d
}

/// 指纹与文本。
fn hash_ok() -> bool {
    let a = u32_text(0) == "0" && u32_text(929) == "929";
    let b = hex_text(0xfe10) == "0x0000fe10" && hex_text(8) == "0x00000008";
    let c = fold_word(FNV_OFF, 0) == digest(&[0u8; 8]);
    let d = digest(b"UNX") != digest(b"UNY");
    let e = fault_text("a", "b") == "a: b";
    let f = VNEED_SIZE == 16 && VER_NDX_HIDDEN == 0x8000;
    a && b && c && d && e && f
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
    let e = l.register(Sample::blank()) == Err(VR_ESEAL);
    let f = l.rejects() == 1 && l.freeze() == Err(VR_ESTATE);
    let g = l.sample(7).map(|s| s.value) == Some(929);
    let h = build_ledger().seal() == root;
    a && b && c && d && e && f && g && h
}

/// 四注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = aux_struct_ok() && section_ok() && offset_ok();
    out[1] = aux_ok() && chain_ok() && count_ok();
    out[2] = flags_ok() && versym_ok() && no_section_ok() && verdef_ok();
    out[3] = trace_ok()
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
pub fn run_unx11628_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11628.verneed_corrupt");

    s.add("VR01-Vernaux 旗标与名字偏移", aux_struct_ok(), "static");
    s.add("VR02-Verneed 条目与节结构", section_ok(), "static");
    s.add("VR03-字符串偏移越界拒载", offset_ok(), "static");
    s.add("VR04-辅助链条数自洽", aux_ok(), "static");
    s.add("VR05-条目链 vn_next 前进", chain_ok(), "static");
    s.add("VR06-DT_VERNEEDNUM 计数", count_ok(), "static");
    s.add("VR07-保留旗标与弱版本", flags_ok(), "static");
    s.add("VR08-versym 索引与隐藏位", versym_ok(), "static");
    s.add("VR09-无版本节与 verdef 共存", no_section_ok() && verdef_ok(), "static");
    s.add("VR10-检查阶段轨迹", trace_ok(), "static");
    s.add("VR11-十四槽语料与渲染", corpus_ok() && slots_ok() && report_ok(), "static");
    s.add("VR12-红档与账本对账", reds_ok() && hash_ok() && pin_ok(), "static");
    s.add("VR13-冻结重放四注入", inject_four() == [true; 4], "static");

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(l.register(Sample::blank()), Err(VR_ESEAL));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn injections() {
        assert_eq!(inject_four(), [true; 4]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11628_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }

    #[test]
    fn text_tools() {
        assert_eq!(u32_text(0).as_str(), "0");
        assert_eq!(u32_text(14).as_str(), "14");
        assert_eq!(hex_text(0xfe10).as_str(), "0x0000fe10");
        assert_eq!(fault_text("a", "b").as_str(), "a: b");
    }

    #[test]
    fn fnv_known() {
        let mut h = FNV_OFF;
        h = fold_byte(h, b'a');
        assert_eq!(h, 0xaf63dc4c8601ec8c);
        assert_ne!(fold_word(FNV_OFF, 2), FNV_OFF);
    }

    #[test]
    fn vernaux_flags() {
        let a = Vernaux::new(1, 0, 2, "G", 13);
        assert!(a.flags_ok());
        assert!(!a.is_weak());
        let w = Vernaux::new(1, VER_FLG_WEAK, 3, "W", 24);
        assert!(w.is_weak() && w.flags_ok());
        let b = Vernaux::new(1, VER_FLG_BASE, 1, "B", 2);
        let i = Vernaux::new(1, VER_FLG_INFO, 4, "I", 4);
        assert!(b.is_base() && i.is_info());
        let bad = Vernaux::new(1, 0x8, 2, "X", 13);
        assert!(!bad.flags_ok());
    }

    #[test]
    fn vernaux_bounds() {
        let a = Vernaux::new(0, 0, 2, "G", 13);
        assert!(a.name_in(128));
        assert!(!a.name_in(13));
        assert!(a.within(2));
        assert!(!a.within(1));
    }

    #[test]
    fn verneed_add_aux() {
        let mut v = Verneed::new("libc.so.6", 1);
        assert_eq!(v.aux_count(), 0);
        assert!(v.count_ok());
        assert!(v.add_aux(Vernaux::new(1, 0, 2, "A", 13)));
        assert_eq!(v.cnt, 1);
        assert!(v.add_aux(Vernaux::new(2, 0, 3, "B", 24)));
        assert_eq!(v.cnt, 2);
        assert!(v.add_aux(Vernaux::new(3, 0, 4, "C", 30)));
        assert!(!v.add_aux(Vernaux::new(4, 0, 5, "D", 35)));
        assert_eq!(v.aux_count(), 3);
        assert!(v.aux_at(2).is_some());
        assert!(v.aux_at(3).is_none());
    }

    #[test]
    fn verneed_aux_ok() {
        let mut v = Verneed::new("libc.so.6", 1);
        assert!(v.aux_ok());
        let _ = v.add_aux(Vernaux::new(1, 0, 2, "A", 13));
        assert!(v.aux_ok());
        v.cnt = 2;
        assert!(!v.aux_ok());
        v.cnt = 1;
        v.aux_off = 0;
        assert!(!v.aux_ok());
    }

    #[test]
    fn section_chain() {
        let mut s = VerSection::with_strsz(128);
        let mut a = Verneed::new("libc.so.6", 1);
        let _ = a.add_aux(Vernaux::new(1, 0, 2, "A", 13));
        let mut b = Verneed::new("libm.so.6", 40);
        let _ = b.add_aux(Vernaux::new(2, 0, 3, "B", 24));
        assert!(s.add(a));
        assert_eq!(s.verneednum, 1);
        assert!(s.add(b));
        assert_eq!(s.verneednum, 2);
        assert!(s.chain_ok());
        assert_eq!(s.entry(0).map(|v| v.next), Some(VNEED_SIZE));
        assert_eq!(s.entry(1).map(|v| v.next), Some(0));
        assert_eq!(s.total_aux(), 2);
    }

    #[test]
    fn section_chain_break() {
        let mut s = VerSection::with_strsz(128);
        let mut a = Verneed::new("libc.so.6", 1);
        let _ = a.add_aux(Vernaux::new(1, 0, 2, "A", 13));
        let mut b = Verneed::new("libm.so.6", 40);
        let _ = b.add_aux(Vernaux::new(2, 0, 3, "B", 24));
        assert!(s.add(a));
        assert!(s.add(b));
        if let Some(v) = s.entries.get_mut(1) {
            v.next = 8;
        }
        assert!(!s.chain_ok());
        assert_eq!(s.chain_break(), Some(1));
    }

    #[test]
    fn section_offsets() {
        let mut s = VerSection::with_strsz(32);
        let mut v = Verneed::new("libc.so.6", 40);
        let _ = v.add_aux(Vernaux::new(1, 0, 2, "GLIBC_2.34", 13));
        assert!(s.add(v));
        assert!(!s.offsets_ok());
        assert_eq!(s.first_bad_offset(), Some(40));
        let mut t = VerSection::with_strsz(64);
        let mut w = Verneed::new("libc.so.6", 1);
        let _ = w.add_aux(Vernaux::new(1, 0, 2, "GLIBC_2.34", 13));
        assert!(t.add(w));
        assert!(t.offsets_ok());
        assert!(t.first_bad_offset().is_none());
    }

    #[test]
    fn section_counts() {
        let mut s = VerSection::with_strsz(64);
        assert!(s.count_ok());
        let mut v = Verneed::new("libc.so.6", 1);
        let _ = v.add_aux(Vernaux::new(1, 0, 2, "A", 13));
        assert!(s.add(v));
        s.verneednum = 2;
        assert!(!s.count_ok());
    }

    #[test]
    fn versym_masking() {
        let mut s = Versym::blank();
        assert_eq!(s.len(), 0);
        assert!(s.push(VER_NDX_LOCAL));
        assert!(s.push(VER_NDX_GLOBAL));
        assert!(s.push(2 | VER_NDX_HIDDEN));
        assert_eq!(s.masked(2), Some(2));
        assert!(s.hidden(2));
        assert!(!s.hidden(0));
        assert!(s.is_local(0));
        assert!(s.is_global(1));
        assert!(s.within(2));
        assert_eq!(s.raw(3), None);
    }

    #[test]
    fn versym_bounds() {
        let mut s = Versym::blank();
        assert!(s.push(5));
        assert!(!s.within(3));
        assert_eq!(s.first_bad(3), Some(5));
        assert_eq!(s.first_bad(5), None);
    }

    #[test]
    fn image_accepts() {
        let j = examine(&image_for(Scenario::SingleAux));
        assert!(j.passed());
        assert_eq!(j.code(), 0);
        let v = match j.view() {
            Some(v) => v,
            None => return,
        };
        assert_eq!(v.files, 1);
        assert_eq!(v.total_aux, 1);
        assert!(!v.has_verdef);
    }

    #[test]
    fn offset_fault() {
        let img = image_for(Scenario::FileOffBad);
        let j = examine(&img);
        assert_eq!(j.code(), VR_EOFF);
        assert!(!j.passed());
        assert_eq!(j.trace.last(), Some((Stage::Offsets, false)));
    }

    #[test]
    fn aux_fault() {
        let img = image_for(Scenario::AuxShort);
        let j = examine(&img);
        assert_eq!(j.code(), VR_EAUX);
        assert_eq!(j.trace.last(), Some((Stage::Aux, false)));
    }

    #[test]
    fn chain_fault() {
        let img = image_for(Scenario::ChainBack);
        let j = examine(&img);
        assert_eq!(j.code(), VR_ELOOP);
        assert!(j.text(&img).contains("does not advance"));
    }

    #[test]
    fn count_fault() {
        let img = image_for(Scenario::CountBad);
        let j = examine(&img);
        assert_eq!(j.code(), VR_ECOUNT);
        assert_eq!(
            j.text(&img).as_str(),
            "./obj: DT_VERNEEDNUM 3 but 2 entries parsed"
        );
    }

    #[test]
    fn versym_fault() {
        let img = image_for(Scenario::VersymBad);
        let j = examine(&img);
        assert_eq!(j.code(), VR_EVERSYM);
        assert_eq!(
            j.text(&img).as_str(),
            "./obj: versym index 5 beyond version count 3"
        );
    }

    #[test]
    fn flag_fault() {
        let img = image_for(Scenario::FlagBad);
        let j = examine(&img);
        assert_eq!(j.code(), VR_EFLAGS);
        assert!(j.text(&img).ends_with("reserved version flag 0x00000008"));
    }

    #[test]
    fn weak_accepted() {
        let img = image_for(Scenario::WeakFlag);
        let j = examine(&img);
        assert!(j.passed());
        let v = match j.view() {
            Some(v) => v,
            None => return,
        };
        assert_eq!(v.weak, 1);
    }

    #[test]
    fn hidden_accepted() {
        let img = image_for(Scenario::HiddenBit);
        let j = examine(&img);
        assert!(j.passed());
        assert!(img.sym.hidden(0));
        assert_eq!(img.sym.masked(0), Some(2));
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
            "verneed corpus=14 accept=8 reject=6 offset=1 chain=2 count=2 flags=1 match=14 rate=1000ppm\n"
        );
    }

    #[test]
    fn red_rate() {
        let r = match run_with(Some((1, VR_EOFF))) {
            Ok(v) => v,
            Err(_) => Report::new(),
        };
        assert_eq!(r.rate(), 929);
        assert_eq!(r.gate(), Err(VR_ESTATE));
    }

    #[test]
    fn cover_prefix() {
        assert_eq!(cover(0), Ok(()));
        assert_eq!(cover(14), Ok(()));
        assert_eq!(cover(15), Err(VR_ESLOT));
    }

    #[test]
    fn error_codes_distinct() {
        let codes = [
            VR_EOFF,
            VR_EAUX,
            VR_ELOOP,
            VR_ECOUNT,
            VR_EVERSYM,
            VR_EFLAGS,
            VR_ESTATE,
            VR_EVER,
            VR_EBAL,
            VR_ESLOT,
            VR_EVALUE,
            VR_ESEAL,
            VR_ENOBS,
        ];
        let mut i = 0usize;
        while i < codes.len() {
            assert_eq!(codes[i], SEG_BASE + 1 + i as u32);
            i += 1;
        }
    }

    #[test]
    fn trace_shape() {
        let j = examine(&image_for(Scenario::SingleAux));
        assert_eq!(j.trace.at(0), Some((Stage::Section, true)));
        assert_eq!(j.trace.at(1), Some((Stage::Offsets, true)));
        assert_eq!(j.trace.last(), Some((Stage::Done, true)));
        assert_eq!(j.trace.len(), 8);
    }
}