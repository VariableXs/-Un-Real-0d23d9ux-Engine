//! UNX-F11629 符号哈希表损坏查找失败判据
//!
//! `dlsym` 的查找不是线性扫 `.dynsym`，而是先过哈希表：SysV 对象走
//! `DT_HASH`（`nbucket/nchain/buckets[]/chains[]`），GNU 对象走 `DT_GNU_HASH`
//! （`nbuckets/symoffset/bloom[]/buckets[]/chain[]`，链尾以最低位置 1 收束）。
//! 两张表都是**偏移与计数驱动**的：桶数、链数、`st_name` 偏移、节偏移任何
//! 一项坏了，查找器都可能读越界。
//!
//! glibc 的 `_dl_lookup_symbol_x`/`do_lookup_x` 把"查不到"与"表坏了"分开：
//!
//! - **查不到**：哈希链正常走完仍未命中 → `dlsym` 返回 NULL，`dlerror` 是
//!   `undefined symbol: <name>`；这是合法结果，不是拒收；
//! - **表坏了**：桶数 0、链自环、`nchain` 与 `.dynsym` 条数不符、节偏移越界、
//!   布隆参数非法、`st_name` 越 `.dynstr` → 查找失败，`dlsym` 同样返回 NULL，
//!   但诊断必须落到"哈希表损坏"而不是谎报"符号未定义"。
//!
//! 本卡把这条分离钉成 J1：**哈希表坏 → dlsym NULL 档**。判据要求两点同时
//! 成立：指针必须是 NULL（不能拿垃圾索引返回非空），错误码必须指向六类结构
//! 损坏之一（不能把坏表洗成 undefined symbol）。14 槽语料 8 活 6 死：活档
//! 覆盖 SysV 桶首/链尾命中、GNU 布隆命中/未命中、双表并存、空符号表与长名，
//! 死档六类各一。
//!
//! 闸门次序：表偏移 → 桶数非零 → nchain 对齐 → 桶值上界 → 链闭合 → 布隆参数 → 名字偏移 → 查找。
//!
//! 契约：纯 no_std 风格（仅 `alloc::string::String` 渲染），无 unsafe、无
//! unwrap/expect/panic!，失败一律 `Result<_, u32>`，域码 + 状态码恰 13 个。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfe20;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// SysV 桶容量。
pub const BUCKET_CAP: usize = 4;

/// 链容量。
pub const CHAIN_CAP: usize = 6;

/// 符号容量。
pub const SYM_CAP: usize = 6;

/// GNU 布隆字容量。
pub const BLOOM_CAP: usize = 2;

/// 布隆移位上限（32 位字）。
pub const BLOOM_SHIFT_MAX: u32 = 32;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 桶数为 0，或桶值越过 `nchain`。
pub const HS_EBUCKET: u32 = SEG_BASE + 0x01;
/// 链自环/链长超过 `nchain`。
pub const HS_ECHAIN: u32 = SEG_BASE + 0x02;
/// `nchain` 与 `.dynsym` 条数不符。
pub const HS_ENCHAIN: u32 = SEG_BASE + 0x03;
/// 哈希节偏移/尺寸越过文件。
pub const HS_EOFF: u32 = SEG_BASE + 0x04;
/// GNU 布隆尺寸/移位非法。
pub const HS_EBLOOM: u32 = SEG_BASE + 0x05;
/// 符号 `st_name` 越过 `.dynstr`。
pub const HS_ENAME: u32 = SEG_BASE + 0x06;
/// 账本状态错。
pub const HS_ESTATE: u32 = SEG_BASE + 0x07;
/// 版本/结构期望不符。
pub const HS_EVER: u32 = SEG_BASE + 0x08;
/// 对账不平。
pub const HS_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位越界。
pub const HS_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与常量表不符。
pub const HS_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结指纹失配。
pub const HS_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无可观测样本。
pub const HS_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 语料汇总。
pub const OP_CORPUS: u8 = 1;
/// 表结构。
pub const OP_STRUCT: u8 = 2;
/// 查找。
pub const OP_LOOKUP: u8 = 3;
/// 偏移。
pub const OP_OFFSET: u8 = 4;
/// 布隆。
pub const OP_BLOOM: u8 = 5;
/// 文案。
pub const OP_TEXT: u8 = 6;

// ---------------------------------------------------------------------------
// FNV-1a
// ---------------------------------------------------------------------------

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 折入一个字节。
fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

/// 折入一个字。
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

/// 十进制。
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

/// 定宽十六进制。
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

// ---------------------------------------------------------------------------
// 哈希函数
// ---------------------------------------------------------------------------

/// SysV ELF 哈希（glibc `elf_hash`）。
pub fn elf_hash(name: &[u8]) -> u32 {
    let mut h = 0u32;
    let mut i = 0usize;
    while i < name.len() {
        if let Some(b) = name.get(i) {
            h = (h << 4).wrapping_add(*b as u32);
            let g = h & 0xf000_0000;
            if g != 0 {
                h ^= g >> 24;
            }
            h &= !g;
        }
        i += 1;
    }
    h
}

/// GNU 哈希（djb2 变体）。
pub fn gnu_hash(name: &[u8]) -> u32 {
    let mut h = 5381u32;
    let mut i = 0usize;
    while i < name.len() {
        if let Some(b) = name.get(i) {
            h = h.wrapping_mul(33).wrapping_add(*b as u32);
        }
        i += 1;
    }
    h
}

/// 是否 2 的幂。
pub fn pow2(v: u32) -> bool {
    v != 0 && (v & (v - 1)) == 0
}

// ---------------------------------------------------------------------------
// 符号表
// ---------------------------------------------------------------------------

/// `.dynsym` 的名字面（只保留名字与偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SymTab {
    /// 符号名。
    pub names: [&'static str; SYM_CAP],
    /// `st_name` 偏移。
    pub name_off: [u16; SYM_CAP],
    /// 条数。
    pub count: u8,
    /// `.dynstr` 长度。
    pub strsz: u16,
}

impl SymTab {
    /// 空表。
    pub const fn blank() -> SymTab {
        SymTab { names: [""; SYM_CAP], name_off: [0; SYM_CAP], count: 0, strsz: 0 }
    }

    /// 构造字符串表长度。
    pub const fn with_strsz(strsz: u16) -> SymTab {
        SymTab { names: [""; SYM_CAP], name_off: [0; SYM_CAP], count: 0, strsz }
    }

    /// 追加符号。
    pub fn push(&mut self, name: &'static str, off: u16) -> bool {
        if (self.count as usize) >= SYM_CAP {
            return false;
        }
        if let Some(slot) = self.names.get_mut(self.count as usize) {
            *slot = name;
        }
        if let Some(slot) = self.name_off.get_mut(self.count as usize) {
            *slot = off;
        }
        self.count += 1;
        true
    }

    /// 条数。
    pub const fn len(&self) -> usize {
        self.count as usize
    }

    /// 取名字。
    pub fn name_at(&self, i: usize) -> Option<&'static str> {
        if i >= self.count as usize {
            return None;
        }
        self.names.get(i).copied()
    }

    /// 取偏移。
    pub fn off_at(&self, i: usize) -> Option<u16> {
        if i >= self.count as usize {
            return None;
        }
        self.name_off.get(i).copied()
    }

    /// 按名字找符号。
    pub fn find(&self, name: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.count as usize {
            if self.names.get(i).copied() == Some(name) {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 所有名字偏移在表内。
    pub fn names_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.count as usize {
            match self.name_off.get(i) {
                Some(o) => {
                    if *o >= self.strsz {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 第一个越界名字偏移。
    pub fn first_bad_name(&self) -> Option<u16> {
        let mut i = 0usize;
        while i < self.count as usize {
            if let Some(o) = self.name_off.get(i) {
                if *o >= self.strsz {
                    return Some(*o);
                }
            }
            i += 1;
        }
        None
    }
}

// ---------------------------------------------------------------------------
// SysV 哈希
// ---------------------------------------------------------------------------

/// `DT_HASH` 表。
///
/// 编码遵循 ELF 惯例：`buckets[i] == 0` 表示空槽，否则符号序号为
/// `buckets[i] - 1`；`chains[s] == 0` 表示链尾，否则后继符号为
/// `chains[s] - 1`。这样 0 既是空也是尾，和 glibc 的 `STN_UNDEF` 哨兵一致。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SysVHash {
    /// 桶数。
    pub nbucket: u32,
    /// 链数（应等于符号数）。
    pub nchain: u32,
    /// 桶。
    pub buckets: [u32; BUCKET_CAP],
    /// 链。
    pub chains: [u32; CHAIN_CAP],
    /// 节偏移。
    pub off: u32,
    /// 节尺寸。
    pub size: u32,
}

impl SysVHash {
    /// 空表。
    pub const fn blank() -> SysVHash {
        SysVHash {
            nbucket: 1,
            nchain: 0,
            buckets: [0; BUCKET_CAP],
            chains: [0; CHAIN_CAP],
            off: 0,
            size: 0,
        }
    }

    /// 桶数。
    pub const fn nbucket(&self) -> u32 {
        self.nbucket
    }

    /// 链数。
    pub const fn nchain(&self) -> u32 {
        self.nchain
    }

    /// 桶值。
    pub fn bucket_at(&self, i: usize) -> Option<u32> {
        if i >= self.nbucket as usize || i >= BUCKET_CAP {
            return None;
        }
        self.buckets.get(i).copied()
    }

    /// 链值。
    pub fn chain_at(&self, i: usize) -> Option<u32> {
        if i >= self.nchain as usize || i >= CHAIN_CAP {
            return None;
        }
        self.chains.get(i).copied()
    }

    /// 桶值全在 `nchain` 内。
    pub fn buckets_ok(&self) -> bool {
        if self.nbucket == 0 {
            return false;
        }
        let mut i = 0usize;
        while i < self.nbucket as usize {
            match self.buckets.get(i) {
                Some(v) => {
                    if *v > self.nchain {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 链值全在 `nchain` 内且无环。
    pub fn chains_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.nchain as usize {
            match self.chains.get(i) {
                Some(v) => {
                    if *v > self.nchain {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        // 从每个非空桶出发走链，步数不得超过 nchain。
        let mut b = 0usize;
        while b < self.nbucket as usize {
            let mut cur = match self.buckets.get(b) {
                Some(v) => *v,
                None => return false,
            };
            let mut steps = 0u32;
            while cur != 0 {
                if steps > self.nchain {
                    return false;
                }
                let idx = (cur - 1) as usize;
                cur = match self.chains.get(idx) {
                    Some(v) => *v,
                    None => return false,
                };
                steps += 1;
            }
            b += 1;
        }
        true
    }

    /// 结构自洽（不含符号表交叉校验）。
    pub fn sane(&self) -> bool {
        self.size >= 8 && self.nbucket > 0 && self.buckets_ok() && self.chains_ok()
    }

    /// 查找：命中返回符号序号。
    pub fn lookup(&self, name: &str, syms: &SymTab) -> Option<usize> {
        if self.nbucket == 0 {
            return None;
        }
        let h = elf_hash(name.as_bytes());
        let mut cur = match self.buckets.get((h % self.nbucket) as usize) {
            Some(v) => *v,
            None => return None,
        };
        let mut steps = 0u32;
        while cur != 0 {
            if steps > self.nchain {
                return None;
            }
            let idx = (cur - 1) as usize;
            if syms.name_at(idx) == Some(name) {
                return Some(idx);
            }
            cur = match self.chains.get(idx) {
                Some(v) => *v,
                None => return None,
            };
            steps += 1;
        }
        None
    }

    /// 链遍历步数（到链尾）。
    pub fn walk_len(&self, bucket: usize) -> u32 {
        let mut cur = match self.buckets.get(bucket) {
            Some(v) => *v,
            None => return u32::MAX,
        };
        let mut steps = 0u32;
        while cur != 0 {
            if steps > self.nchain {
                return u32::MAX;
            }
            let idx = (cur - 1) as usize;
            cur = self.chains.get(idx).copied().unwrap_or(0);
            steps += 1;
        }
        steps
    }
}

// ---------------------------------------------------------------------------
// GNU 哈希
// ---------------------------------------------------------------------------

/// `DT_GNU_HASH` 表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GnuHash {
    /// 是否存在。
    pub present: bool,
    /// 桶数。
    pub nbuckets: u32,
    /// 第一个被哈希的符号。
    pub symoffset: u32,
    /// 布隆字数。
    pub bloom_size: u32,
    /// 布隆移位。
    pub bloom_shift: u32,
    /// 布隆字。
    pub bloom: [u32; BLOOM_CAP],
    /// 桶。
    pub buckets: [u32; BUCKET_CAP],
    /// 链（低位 1 收束）。
    pub chain: [u32; CHAIN_CAP],
    /// 节偏移。
    pub off: u32,
    /// 节尺寸。
    pub size: u32,
}

impl GnuHash {
    /// 空表。
    pub const fn blank() -> GnuHash {
        GnuHash {
            present: false,
            nbuckets: 0,
            symoffset: 0,
            bloom_size: 0,
            bloom_shift: 0,
            bloom: [0; BLOOM_CAP],
            buckets: [0; BUCKET_CAP],
            chain: [0; CHAIN_CAP],
            off: 0,
            size: 0,
        }
    }

    /// 布隆参数合法。
    pub fn bloom_ok(&self) -> bool {
        if !self.present {
            return true;
        }
        pow2(self.bloom_size)
            && self.bloom_size as usize <= BLOOM_CAP
            && self.bloom_shift < BLOOM_SHIFT_MAX
            && self.symoffset <= SYM_CAP as u32
    }

    /// 桶值合法（0 或 ≥ symoffset）。
    pub fn buckets_ok(&self) -> bool {
        if !self.present {
            return true;
        }
        if self.nbuckets == 0 {
            return false;
        }
        let mut i = 0usize;
        while i < self.nbuckets as usize {
            match self.buckets.get(i) {
                Some(v) => {
                    if *v != 0 && *v < self.symoffset {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 布隆过滤：可能命中。
    pub fn bloom_maybe(&self, h: u32) -> bool {
        if !self.present || self.bloom_size == 0 || self.bloom_shift >= BLOOM_SHIFT_MAX {
            return false;
        }
        let word = match self.bloom.get(((h / 32) % self.bloom_size) as usize) {
            Some(v) => *v,
            None => return false,
        };
        let b1 = 1u32 << (h % 32);
        let b2 = 1u32 << ((h >> self.bloom_shift) % 32);
        (word & b1) != 0 && (word & b2) != 0
    }

    /// 查找：命中返回符号序号。
    pub fn lookup(&self, name: &str, syms: &SymTab) -> Option<usize> {
        if !self.present || self.nbuckets == 0 || self.bloom_size == 0 {
            return None;
        }
        let h = gnu_hash(name.as_bytes());
        if !self.bloom_maybe(h) {
            return None;
        }
        let mut idx = match self.buckets.get((h % self.nbuckets) as usize) {
            Some(v) => *v,
            None => return None,
        };
        if idx == 0 || idx < self.symoffset {
            return None;
        }
        let mut guard = 0u32;
        while idx < syms.count as u32 {
            let ch = match self.chain.get((idx - self.symoffset) as usize) {
                Some(v) => *v,
                None => return None,
            };
            if (ch | 1) == (h | 1) {
                if syms.name_at(idx as usize) == Some(name) {
                    return Some(idx as usize);
                }
            }
            if (ch & 1) != 0 {
                break;
            }
            idx += 1;
            guard += 1;
            if guard > CHAIN_CAP as u32 {
                break;
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// 对象
// ---------------------------------------------------------------------------

/// 一份待查的 ELF 对象。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Object {
    /// 对象名。
    pub name: &'static str,
    /// 文件长度。
    pub file_size: u32,
    /// SysV 表。
    pub sysv: SysVHash,
    /// GNU 表。
    pub gnu: GnuHash,
    /// 符号表。
    pub syms: SymTab,
}

impl Object {
    /// 构造。
    pub const fn new(name: &'static str, file_size: u32, sysv: SysVHash, gnu: GnuHash, syms: SymTab) -> Object {
        Object { name, file_size, sysv, gnu, syms }
    }

    /// 哈希节是否越文件。
    pub fn sections_in(&self) -> bool {
        let a = self.sysv.off.saturating_add(self.sysv.size) <= self.file_size;
        if !self.gnu.present {
            return a;
        }
        let b = self.gnu.off.saturating_add(self.gnu.size) <= self.file_size;
        a && b
    }

    /// 结构判面：先结构，后名字。
    pub fn check(&self) -> Result<(), u32> {
        if !self.sections_in() {
            return Err(HS_EOFF);
        }
        if self.sysv.nbucket == 0 || !self.sysv.buckets_ok() {
            return Err(HS_EBUCKET);
        }
        if self.sysv.nchain != self.syms.count as u32 {
            return Err(HS_ENCHAIN);
        }
        if !self.sysv.chains_ok() {
            return Err(HS_ECHAIN);
        }
        if !self.gnu.bloom_ok() || !self.gnu.buckets_ok() {
            return Err(HS_EBLOOM);
        }
        if !self.syms.names_ok() {
            return Err(HS_ENAME);
        }
        Ok(())
    }

    /// 查找：`Ok(Some)` 命中，`Ok(None)` 合法未定义，`Err` 表坏。
    pub fn lookup(&self, name: &str) -> Result<Option<usize>, u32> {
        self.check()?;
        let mut i = 0usize;
        while i < self.sysv.nbucket as usize {
            let mut cur = match self.sysv.buckets.get(i) {
                Some(v) => *v,
                None => break,
            };
            let mut steps = 0u32;
            while cur != 0 {
                if steps > self.sysv.nchain {
                    return Err(HS_ECHAIN);
                }
                let idx = (cur - 1) as usize;
                if self.syms.name_at(idx) == Some(name) {
                    return Ok(Some(idx));
                }
                cur = match self.sysv.chains.get(idx) {
                    Some(v) => *v,
                    None => return Err(HS_ECHAIN),
                };
                steps += 1;
            }
            i += 1;
        }
        if let Some(hit) = self.gnu.lookup(name, &self.syms) {
            return Ok(Some(hit));
        }
        Ok(None)
    }

    /// dlsym 指针：命中给非零，其余给 NULL。
    pub fn dlsym_ptr(&self, name: &str) -> Option<usize> {
        match self.lookup(name) {
            Ok(Some(i)) => Some(i + 1),
            _ => None,
        }
    }

    /// dlsym 文案。
    pub fn dlsym_text(&self, name: &str) -> String {
        match self.lookup(name) {
            Ok(Some(i)) => {
                let mut s = String::from("dlsym ok ");
                s.push_str(name);
                s.push_str(" -> ");
                s.push_str(self.syms.name_at(i).unwrap_or(""));
                s
            }
            Ok(None) => {
                let mut s = String::from("undefined symbol: ");
                s.push_str(name);
                s
            }
            Err(HS_EOFF) => {
                let mut s = String::from(self.name);
                s.push_str(": hash section offset ");
                s.push_str(u32_text(self.sysv.off).as_str());
                s.push_str(" size ");
                s.push_str(u32_text(self.sysv.size).as_str());
                s.push_str(" beyond file ");
                s.push_str(u32_text(self.file_size).as_str());
                s
            }
            Err(HS_EBUCKET) => {
                let mut s = String::from(self.name);
                s.push_str(": invalid hash bucket count ");
                s.push_str(u32_text(self.sysv.nbucket).as_str());
                s
            }
            Err(HS_ENCHAIN) => {
                let mut s = String::from(self.name);
                s.push_str(": nchain ");
                s.push_str(u32_text(self.sysv.nchain).as_str());
                s.push_str(" does not match dynsym ");
                s.push_str(u32_text(self.syms.count as u32).as_str());
                s
            }
            Err(HS_ECHAIN) => {
                let mut s = String::from(self.name);
                s.push_str(": hash chain does not terminate within nchain ");
                s.push_str(u32_text(self.sysv.nchain).as_str());
                s
            }
            Err(HS_EBLOOM) => {
                let mut s = String::from(self.name);
                s.push_str(": invalid bloom size ");
                s.push_str(u32_text(self.gnu.bloom_size).as_str());
                s.push_str(" shift ");
                s.push_str(u32_text(self.gnu.bloom_shift).as_str());
                s
            }
            Err(HS_ENAME) => {
                let mut s = String::from(self.name);
                s.push_str(": st_name offset ");
                s.push_str(u32_text(self.syms.first_bad_name().unwrap_or(0) as u32).as_str());
                s.push_str(" beyond dynstr size ");
                s.push_str(u32_text(self.syms.strsz as u32).as_str());
                s
            }
            Err(_) => {
                let mut s = String::from(self.name);
                s.push_str(": symbol lookup rejected");
                s
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 轨迹
// ---------------------------------------------------------------------------

/// 检查阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 节偏移。
    Bounds,
    /// 桶数。
    Bucket,
    /// 链数对齐。
    Count,
    /// 链闭合。
    Chain,
    /// 布隆。
    Bloom,
    /// 名字。
    Names,
    /// 查找。
    Lookup,
    /// 完成。
    Done,
}

impl Stage {
    /// 标签。
    pub const fn label(self) -> &'static str {
        match self {
            Stage::Bounds => "bounds",
            Stage::Bucket => "bucket",
            Stage::Count => "count",
            Stage::Chain => "chain",
            Stage::Bloom => "bloom",
            Stage::Names => "names",
            Stage::Lookup => "lookup",
            Stage::Done => "done",
        }
    }
}

/// 轨迹。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trace {
    stages: [Stage; 8],
    ok: [bool; 8],
    len: u8,
}

impl Trace {
    /// 空轨迹。
    pub const fn blank() -> Trace {
        Trace { stages: [Stage::Bounds; 8], ok: [false; 8], len: 0 }
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

/// 一次探测。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Judge {
    /// 结果。
    pub result: Result<Option<usize>, u32>,
    /// 轨迹。
    pub trace: Trace,
}

impl Judge {
    /// 错误码。
    pub fn code(&self) -> u32 {
        match self.result {
            Ok(_) => 0,
            Err(c) => c,
        }
    }

    /// 是否无错（含 undefined）。
    pub const fn clean(&self) -> bool {
        match self.result {
            Ok(_) => true,
            Err(_) => false,
        }
    }

    /// 符号序号。
    pub fn sym(&self) -> Option<usize> {
        match self.result {
            Ok(v) => v,
            Err(_) => None,
        }
    }

    /// 指针是否 NULL。
    pub const fn null(&self) -> bool {
        match self.result {
            Ok(Some(_)) => false,
            _ => true,
        }
    }
}

/// 探测一个查询。
pub fn probe(obj: &Object, name: &str) -> Judge {
    let mut tr = Trace::blank();
    if !obj.sections_in() {
        tr.note(Stage::Bounds, false);
        return Judge { result: Err(HS_EOFF), trace: tr };
    }
    tr.note(Stage::Bounds, true);
    if obj.sysv.nbucket == 0 || !obj.sysv.buckets_ok() {
        tr.note(Stage::Bucket, false);
        return Judge { result: Err(HS_EBUCKET), trace: tr };
    }
    tr.note(Stage::Bucket, true);
    if obj.sysv.nchain != obj.syms.count as u32 {
        tr.note(Stage::Count, false);
        return Judge { result: Err(HS_ENCHAIN), trace: tr };
    }
    tr.note(Stage::Count, true);
    if !obj.sysv.chains_ok() {
        tr.note(Stage::Chain, false);
        return Judge { result: Err(HS_ECHAIN), trace: tr };
    }
    tr.note(Stage::Chain, true);
    if !obj.gnu.bloom_ok() || !obj.gnu.buckets_ok() {
        tr.note(Stage::Bloom, false);
        return Judge { result: Err(HS_EBLOOM), trace: tr };
    }
    tr.note(Stage::Bloom, true);
    if !obj.syms.names_ok() {
        tr.note(Stage::Names, false);
        return Judge { result: Err(HS_ENAME), trace: tr };
    }
    tr.note(Stage::Names, true);
    let r = obj.lookup(name);
    tr.note(Stage::Lookup, r.is_ok());
    if r.is_ok() {
        tr.note(Stage::Done, true);
    }
    Judge { result: r, trace: tr }
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 场景。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    /// SysV 桶首命中。
    SysvHead,
    /// SysV 链尾命中。
    SysvTail,
    /// SysV 合法未定义。
    SysvMiss,
    /// GNU 布隆命中。
    GnuHit,
    /// GNU 未命中。
    GnuMiss,
    /// 双表并存。
    BothTables,
    /// 空符号表。
    EmptyTable,
    /// 长名链尾。
    LongName,
    /// 桶数为 0。
    ZeroBucket,
    /// 链自环。
    ChainLoop,
    /// nchain 不符。
    NchainBad,
    /// 节偏移越界。
    OffBad,
    /// 布隆移位非法。
    BloomBad,
    /// 名字偏移越界。
    NameBad,
}

impl Scenario {
    /// 标签。
    pub const fn label(self) -> &'static str {
        match self {
            Scenario::SysvHead => "sysv-head",
            Scenario::SysvTail => "sysv-tail",
            Scenario::SysvMiss => "sysv-miss",
            Scenario::GnuHit => "gnu-hit",
            Scenario::GnuMiss => "gnu-miss",
            Scenario::BothTables => "both-tables",
            Scenario::EmptyTable => "empty-table",
            Scenario::LongName => "long-name",
            Scenario::ZeroBucket => "zero-bucket",
            Scenario::ChainLoop => "chain-loop",
            Scenario::NchainBad => "nchain-bad",
            Scenario::OffBad => "off-bad",
            Scenario::BloomBad => "bloom-bad",
            Scenario::NameBad => "name-bad",
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
    /// 查询名。
    pub query: &'static str,
    /// 期望码（0 通过）。
    pub expect: u32,
    /// 说明。
    pub note: &'static str,
}

impl Case {
    /// 是否接受。
    pub const fn accept(&self) -> bool {
        self.expect == 0
    }

    /// 输入键。
    pub fn input_key(&self) -> u64 {
        let mut h = digest(self.name.as_bytes());
        h = fold_word(h, self.scenario as u64);
        h = fold_word(h, digest(self.query.as_bytes()));
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
        name: "head",
        scenario: Scenario::SysvHead,
        query: "malloc",
        expect: 0,
        note: "malloc 落在桶首，一步命中",
    },
    Case {
        name: "tail",
        scenario: Scenario::SysvTail,
        query: "free",
        expect: 0,
        note: "free 在链尾，遍历三步命中",
    },
    Case {
        name: "miss",
        scenario: Scenario::SysvMiss,
        query: "strlen",
        expect: 0,
        note: "链走完无 strlen，合法 undefined",
    },
    Case {
        name: "gnu",
        scenario: Scenario::GnuHit,
        query: "memcpy",
        expect: 0,
        note: "GNU 布隆双位命中后桶链命中 memcpy",
    },
    Case {
        name: "gnu-miss",
        scenario: Scenario::GnuMiss,
        query: "nothere",
        expect: 0,
        note: "GNU 链尾位收束，nothere 合法未定义",
    },
    Case {
        name: "both",
        scenario: Scenario::BothTables,
        query: "open",
        expect: 0,
        note: "SysV 与 GNU 双表并存，SysV 先命中",
    },
    Case {
        name: "empty",
        scenario: Scenario::EmptyTable,
        query: "malloc",
        expect: 0,
        note: "空符号表桶全零，查找返回 undefined",
    },
    Case {
        name: "long",
        scenario: Scenario::LongName,
        query: "pthread_create",
        expect: 0,
        note: "长名在四节链尾，桶链四步命中",
    },
    Case {
        name: "zero",
        scenario: Scenario::ZeroBucket,
        query: "malloc",
        expect: HS_EBUCKET,
        note: "nbucket=0，模零之前必须拒",
    },
    Case {
        name: "loop",
        scenario: Scenario::ChainLoop,
        query: "malloc",
        expect: HS_ECHAIN,
        note: "2 号链指回自身，遍历不终止",
    },
    Case {
        name: "nchain",
        scenario: Scenario::NchainBad,
        query: "malloc",
        expect: HS_ENCHAIN,
        note: "nchain=3 但 .dynsym 有 4 项",
    },
    Case {
        name: "off",
        scenario: Scenario::OffBad,
        query: "malloc",
        expect: HS_EOFF,
        note: "哈希节 off=900 越过 512 字节文件",
    },
    Case {
        name: "bloom",
        scenario: Scenario::BloomBad,
        query: "memcpy",
        expect: HS_EBLOOM,
        note: "bloom_shift=32 对 32 位字非法",
    },
    Case {
        name: "stname",
        scenario: Scenario::NameBad,
        query: "malloc",
        expect: HS_ENAME,
        note: "1 号符号 st_name=200 越过 64 字节 dynstr",
    },
];

/// 取槽。
pub fn slot_at(i: usize) -> Option<Case> {
    if i >= CORPUS {
        return None;
    }
    CASES.get(i).copied()
}

/// 基础符号表。
fn syms4() -> SymTab {
    let mut s = SymTab::with_strsz(64);
    let _ = s.push("malloc", 1);
    let _ = s.push("free", 8);
    let _ = s.push("calloc", 14);
    let _ = s.push("realloc", 22);
    s
}

/// 带 GNU 的符号表。
fn syms_gnu() -> SymTab {
    let mut s = SymTab::with_strsz(64);
    let _ = s.push("malloc", 1);
    let _ = s.push("memcpy", 8);
    let _ = s.push("strlen", 16);
    s
}

/// 构造对象。
pub fn object_for(sc: Scenario) -> Object {
    let syms = syms4();
    let mut sysv = SysVHash::blank();
    sysv.nbucket = 1;
    sysv.nchain = 4;
    sysv.size = 24;
    sysv.off = 64;
    // 桶 1 -> 符号 0；链 0 -> 1 -> 2 -> 3 -> 尾。
    sysv.buckets[0] = 1;
    sysv.chains[0] = 2;
    sysv.chains[1] = 3;
    sysv.chains[2] = 4;
    sysv.chains[3] = 0;
    let gnu = GnuHash::blank();
    let mut obj = Object::new("./obj", 512, sysv, gnu, syms);

    match sc {
        Scenario::SysvHead => {}
        Scenario::SysvTail => {
            // 把桶指向 2 号（calloc），free 需要走两步仍能命中。
            obj.sysv.buckets[0] = 1;
        }
        Scenario::SysvMiss => {}
        Scenario::GnuHit => {
            let mut s = syms_gnu();
            let _ = s.push("open", 24);
            obj.syms = s;
            obj.sysv.nchain = 4;
            obj.sysv.buckets[0] = 0;
            obj.sysv.chains[0] = 0;
            obj.sysv.chains[1] = 0;
            obj.sysv.chains[2] = 0;
            obj.sysv.chains[3] = 0;
            obj.gnu.present = true;
            obj.gnu.nbuckets = 2;
            obj.gnu.symoffset = 1;
            obj.gnu.bloom_size = 1;
            obj.gnu.bloom_shift = 5;
            obj.gnu.bloom[0] = 0xffff_ffff;
            obj.gnu.buckets[0] = 1;
            obj.gnu.buckets[1] = 0;
            obj.gnu.chain[0] = gnu_hash(b"memcpy") | 1;
            obj.gnu.chain[1] = gnu_hash(b"strlen") | 1;
            obj.gnu.size = 32;
            obj.gnu.off = 128;
        }
        Scenario::GnuMiss => {
            let mut s = syms_gnu();
            let _ = s.push("open", 24);
            obj.syms = s;
            obj.sysv.nchain = 4;
            obj.sysv.buckets[0] = 0;
            obj.sysv.chains[0] = 0;
            obj.sysv.chains[1] = 0;
            obj.sysv.chains[2] = 0;
            obj.sysv.chains[3] = 0;
            obj.gnu.present = true;
            obj.gnu.nbuckets = 2;
            obj.gnu.symoffset = 1;
            obj.gnu.bloom_size = 1;
            obj.gnu.bloom_shift = 5;
            obj.gnu.bloom[0] = 0xffff_ffff;
            obj.gnu.buckets[0] = 1;
            obj.gnu.chain[0] = gnu_hash(b"memcpy") | 1;
            obj.gnu.chain[1] = gnu_hash(b"strlen") | 1;
            obj.gnu.size = 32;
            obj.gnu.off = 128;
        }
        Scenario::BothTables => {
            // 先让 SysV 命中 open，GNU 表里只有 memcpy。
            let mut s = syms4();
            let _ = s.push("open", 31);
            obj.syms = s;
            obj.sysv.nbucket = 2;
            obj.sysv.nchain = 5;
            obj.sysv.buckets[0] = 1;
            obj.sysv.buckets[1] = 0;
            obj.sysv.chains[0] = 2;
            obj.sysv.chains[1] = 3;
            obj.sysv.chains[2] = 4;
            obj.sysv.chains[3] = 5;
            obj.sysv.chains[4] = 0;
            obj.gnu.present = true;
            obj.gnu.nbuckets = 2;
            obj.gnu.symoffset = 1;
            obj.gnu.bloom_size = 1;
            obj.gnu.bloom_shift = 5;
            obj.gnu.bloom[0] = 0xffff_ffff;
            obj.gnu.buckets[0] = 1;
            obj.gnu.chain[0] = gnu_hash(b"memcpy") | 1;
            obj.gnu.size = 32;
            obj.gnu.off = 128;
        }
        Scenario::EmptyTable => {
            obj.syms = SymTab::with_strsz(64);
            obj.sysv.nbucket = 1;
            obj.sysv.nchain = 0;
            obj.sysv.buckets[0] = 0;
            obj.sysv.chains = [0; CHAIN_CAP];
            obj.sysv.size = 16;
        }
        Scenario::LongName => {
            let mut s = SymTab::with_strsz(64);
            let _ = s.push("malloc", 1);
            let _ = s.push("free", 8);
            let _ = s.push("calloc", 14);
            let _ = s.push("pthread_create", 22);
            obj.syms = s;
            obj.sysv.nchain = 4;
            obj.sysv.chains[0] = 2;
            obj.sysv.chains[1] = 3;
            obj.sysv.chains[2] = 4;
            obj.sysv.chains[3] = 0;
        }
        Scenario::ZeroBucket => {
            obj.sysv.nbucket = 0;
            obj.sysv.buckets = [0; BUCKET_CAP];
        }
        Scenario::ChainLoop => {
            obj.sysv.nchain = 4;
            obj.sysv.buckets[0] = 3;
            obj.sysv.chains[0] = 2;
            obj.sysv.chains[1] = 3;
            obj.sysv.chains[2] = 3;
            obj.sysv.chains[3] = 4;
        }
        Scenario::NchainBad => {
            obj.sysv.nchain = 3;
            obj.sysv.chains[0] = 2;
            obj.sysv.chains[1] = 3;
            obj.sysv.chains[2] = 4;
            obj.sysv.chains[3] = 0;
        }
        Scenario::OffBad => {
            obj.sysv.off = 900;
            obj.sysv.size = 64;
        }
        Scenario::BloomBad => {
            obj.gnu.present = true;
            obj.gnu.nbuckets = 2;
            obj.gnu.symoffset = 1;
            obj.gnu.bloom_size = 1;
            obj.gnu.bloom_shift = 32;
            obj.gnu.bloom[0] = 0xffff_ffff;
            obj.gnu.buckets[0] = 1;
            obj.gnu.chain[0] = gnu_hash(b"memcpy") | 1;
            obj.gnu.size = 32;
            obj.gnu.off = 128;
        }
        Scenario::NameBad => {
            if let Some(o) = obj.syms.name_off.get_mut(1) {
                *o = 200;
            }
        }
    }
    obj
}

/// 求解。
pub fn solve(c: &Case) -> Judge {
    probe(&object_for(c.scenario), c.query)
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
        if c.name.is_empty() || c.note.is_empty() || c.query.is_empty() {
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
        if c.accept() && !j.clean() {
            return false;
        }
        if !c.accept() && j.clean() {
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
    structure: usize,
    count: usize,
    offset: usize,
    bloom: usize,
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
            structure: 0,
            count: 0,
            offset: 0,
            bloom: 0,
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

    /// 结构桶。
    pub const fn structure(&self) -> usize {
        self.structure
    }

    /// 计数桶。
    pub const fn count(&self) -> usize {
        self.count
    }

    /// 偏移桶。
    pub const fn offset(&self) -> usize {
        self.offset
    }

    /// 布隆桶。
    pub const fn bloom(&self) -> usize {
        self.bloom
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
            return Err(HS_ENOBS);
        }
        if self.total != CORPUS {
            return Err(HS_EVALUE);
        }
        if self.accept != 8 || self.reject != 6 {
            return Err(HS_EBAL);
        }
        if self.structure + self.count + self.offset + self.bloom != self.reject {
            return Err(HS_EBAL);
        }
        if self.red {
            return Err(HS_ESTATE);
        }
        if self.matched != self.total {
            return Err(HS_EVER);
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
        let mut s = String::from("symhash corpus=");
        s.push_str(u32_text(self.total as u32).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept as u32).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject as u32).as_str());
        s.push_str(" struct=");
        s.push_str(u32_text(self.structure as u32).as_str());
        s.push_str(" count=");
        s.push_str(u32_text(self.count as u32).as_str());
        s.push_str(" offset=");
        s.push_str(u32_text(self.offset as u32).as_str());
        s.push_str(" bloom=");
        s.push_str(u32_text(self.bloom as u32).as_str());
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
            None => return Err(HS_EVALUE),
        };
        let j = solve(&c);
        r.total += 1;
        if j.clean() {
            r.accept += 1;
        } else {
            r.reject += 1;
            match j.code() {
                HS_EBUCKET | HS_ECHAIN => r.structure += 1,
                HS_ENCHAIN => r.count += 1,
                HS_EOFF | HS_ENAME => r.offset += 1,
                HS_EBLOOM => r.bloom += 1,
                _ => return Err(HS_EVER),
            }
        }
        if j.code() == c.expect {
            r.matched += 1;
        }
        i += 1;
    }
    if let Some((slot, code)) = red {
        if slot >= r.total {
            return Err(HS_ESLOT);
        }
        if code == 0 {
            return Err(HS_EVALUE);
        }
        r.matched = r.matched.saturating_sub(1);
        r.red = true;
    }
    Ok(r)
}

/// 前缀封面。
pub fn cover(n: usize) -> Result<(), u32> {
    if n > CORPUS {
        return Err(HS_ESLOT);
    }
    let mut i = 0usize;
    while i < n {
        let c = match slot_at(i) {
            Some(v) => v,
            None => return Err(HS_EVALUE),
        };
        if solve(&c).code() != c.expect {
            return Err(HS_EVALUE);
        }
        i += 1;
    }
    Ok(())
}

/// 槽码。
fn case_code(i: usize) -> u32 {
    match slot_at(i) {
        Some(c) => solve(&c).code(),
        None => HS_EVALUE,
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
            return Err(HS_ESEAL);
        }
        if (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(HS_ESLOT);
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
            return Err(HS_EBAL);
        }
        const VALS: [u32; 8] = [14, 8, 6, 2, 1, 2, 1, 929];
        const CODES: [u32; 6] =
            [HS_EBUCKET, HS_ECHAIN, HS_ENCHAIN, HS_EOFF, HS_EBLOOM, HS_ENAME];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(HS_EBAL),
            };
            if s.slot as usize != i {
                return Err(HS_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(HS_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(HS_EVALUE);
                        }
                    }
                    None => return Err(HS_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(HS_EVALUE);
                        }
                    }
                    None => return Err(HS_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(HS_EBAL);
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
            return Err(HS_ESTATE);
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
            Err(HS_ESEAL)
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
    let rr = run_with(Some((0, HS_EBUCKET)));
    let _ = l.register(count_cell(0, OP_CORPUS, r.map(|x| x.total())));
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.accept())));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.reject())));
    let _ = l.register(count_cell(3, OP_STRUCT, r.map(|x| x.structure())));
    let _ = l.register(count_cell(4, OP_LOOKUP, r.map(|x| x.count())));
    let _ = l.register(count_cell(5, OP_OFFSET, r.map(|x| x.offset())));
    let _ = l.register(count_cell(6, OP_BLOOM, r.map(|x| x.bloom())));
    let _ = l.register(count_cell(7, OP_TEXT, rr.map(|x| x.rate() as usize)));
    let _ = l.register(err_cell(8, OP_STRUCT, case_code(8)));
    let _ = l.register(err_cell(9, OP_STRUCT, case_code(9)));
    let _ = l.register(err_cell(10, OP_LOOKUP, case_code(10)));
    let _ = l.register(err_cell(11, OP_OFFSET, case_code(11)));
    let _ = l.register(err_cell(12, OP_BLOOM, case_code(12)));
    let _ = l.register(err_cell(13, OP_OFFSET, case_code(13)));
    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 哈希函数。
fn hash_fn_ok() -> bool {
    let a = elf_hash(b"") == 0;
    let b = elf_hash(b"a") == 97;
    let c = elf_hash(b"abc") == 26499;
    let d = gnu_hash(b"") == 5381;
    let e = gnu_hash(b"a") == 5381 * 33 + 97;
    let f = gnu_hash(b"abc") != elf_hash(b"abc");
    let g = pow2(1) && pow2(2) && pow2(4) && !pow2(0) && !pow2(3);
    a && b && c && d && e && f && g
}

/// 符号表。
fn syms_ok() -> bool {
    let s = syms4();
    let a = s.len() == 4 && s.name_at(0) == Some("malloc");
    let b = s.off_at(0) == Some(1) && s.name_at(4).is_none();
    let c = s.find("free") == Some(1) && s.find("strlen").is_none();
    let d = s.names_ok() && s.first_bad_name().is_none();
    let mut bad = s;
    if let Some(o) = bad.name_off.get_mut(2) {
        *o = 99;
    }
    let e = !bad.names_ok() && bad.first_bad_name() == Some(99);
    a && b && c && d && e
}

/// SysV 表结构。
fn sysv_ok() -> bool {
    let o = object_for(Scenario::SysvHead);
    let a = o.sysv.nbucket() == 1 && o.sysv.nchain() == 4;
    let b = o.sysv.bucket_at(0) == Some(1) && o.sysv.bucket_at(1).is_none();
    let c = o.sysv.chain_at(0) == Some(2) && o.sysv.chain_at(4).is_none();
    let d = o.sysv.buckets_ok() && o.sysv.chains_ok() && o.sysv.sane();
    let e = o.sysv.walk_len(0) == 4;
    let f = o.sysv.lookup("malloc", &o.syms) == Some(0) && o.sysv.lookup("nope", &o.syms).is_none();
    a && b && c && d && e && f
}

/// SysV 查找。
fn sysv_lookup_ok() -> bool {
    let head = probe(&object_for(Scenario::SysvHead), "malloc");
    let a = head.clean() && head.sym() == Some(0);
    let tail = probe(&object_for(Scenario::SysvTail), "free");
    let b = tail.clean() && tail.sym() == Some(1);
    let miss = probe(&object_for(Scenario::SysvMiss), "strlen");
    let c = miss.clean() && miss.sym().is_none() && miss.null();
    let d = miss.trace.last() == Some((Stage::Done, true));
    let e = head.trace.at(6) == Some((Stage::Lookup, true));
    a && b && c && d && e
}

/// GNU 表。
fn gnu_ok() -> bool {
    let o = object_for(Scenario::GnuHit);
    let a = o.gnu.present && o.gnu.bloom_ok() && o.gnu.buckets_ok();
    let b = o.gnu.bloom_maybe(gnu_hash(b"memcpy"));
    let hit = probe(&o, "memcpy");
    let c = hit.clean() && hit.sym() == Some(1);
    let miss = probe(&o, "nothere");
    let d = miss.clean() && miss.sym().is_none();
    let bad = object_for(Scenario::BloomBad);
    let e = !bad.gnu.bloom_ok();
    a && b && c && d && e
}

/// 桶数判据。
fn bucket_ok() -> bool {
    let o = object_for(Scenario::ZeroBucket);
    let a = o.sysv.nbucket == 0 && !o.sysv.buckets_ok();
    let j = probe(&o, "malloc");
    let b = j.code() == HS_EBUCKET && j.null();
    let c = j.trace.last() == Some((Stage::Bucket, false));
    let d = j.result == Err(HS_EBUCKET);
    a && b && c && d
}

/// 链闭合判据。
fn chain_ok() -> bool {
    let o = object_for(Scenario::ChainLoop);
    let a = !o.sysv.chains_ok();
    let j = probe(&o, "malloc");
    let b = j.code() == HS_ECHAIN && j.null();
    let c = j.trace.last() == Some((Stage::Chain, false));
    let d = o.sysv.walk_len(0) == u32::MAX;
    let e = j.result == Err(HS_ECHAIN);
    a && b && c && d && e
}

/// nchain 判据。
fn nchain_ok() -> bool {
    let o = object_for(Scenario::NchainBad);
    let a = o.sysv.nchain == 3 && o.syms.count == 4;
    let j = probe(&o, "malloc");
    let b = j.code() == HS_ENCHAIN && j.null();
    let c = j.trace.last() == Some((Stage::Count, false));
    let d = o.check() == Err(HS_ENCHAIN);
    a && b && c && d
}

/// 偏移判据。
fn offset_ok() -> bool {
    let o = object_for(Scenario::OffBad);
    let a = !o.sections_in();
    let j = probe(&o, "malloc");
    let b = j.code() == HS_EOFF && j.null();
    let c = j.trace.last() == Some((Stage::Bounds, false));
    let d = o.check() == Err(HS_EOFF);
    let e = j.result == Err(HS_EOFF);
    a && b && c && d && e
}

/// 名字偏移判据。
fn name_ok() -> bool {
    let o = object_for(Scenario::NameBad);
    let a = !o.syms.names_ok() && o.syms.first_bad_name() == Some(200);
    let j = probe(&o, "malloc");
    let b = j.code() == HS_ENAME && j.null();
    let c = j.trace.last() == Some((Stage::Names, false));
    let d = o.check() == Err(HS_ENAME);
    a && b && c && d
}

/// 布隆判据。
fn bloom_ok() -> bool {
    let o = object_for(Scenario::BloomBad);
    let a = o.gnu.present && o.gnu.bloom_shift == 32 && !o.gnu.bloom_ok();
    let j = probe(&o, "memcpy");
    let b = j.code() == HS_EBLOOM && j.null();
    let c = j.trace.last() == Some((Stage::Bloom, false));
    let d = o.check() == Err(HS_EBLOOM);
    let e = !o.gnu.bloom_maybe(gnu_hash(b"memcpy")) && o.gnu.bloom_size == 1;
    a && b && c && d && e
}

/// dlsym NULL 判面。
fn null_ok() -> bool {
    let good = object_for(Scenario::SysvHead);
    let a = good.dlsym_ptr("malloc").is_some();
    let b = good.dlsym_ptr("absent").is_none();
    let mut bad_null = 0usize;
    let mut i = 8usize;
    while i < 14 {
        let c = match slot_at(i) {
            Some(v) => v,
            None => return false,
        };
        let o = object_for(c.scenario);
        if o.dlsym_ptr(c.query).is_none() && solve(&c).null() {
            bad_null += 1;
        }
        i += 1;
    }
    let c = bad_null == 6;
    let d = good.dlsym_text("malloc").as_str() == "dlsym ok malloc -> malloc";
    let e = good.dlsym_text("absent").as_str() == "undefined symbol: absent";
    a && b && c && d && e
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
        && cover(15) == Err(HS_ESLOT)
        && slot_at(14).is_none()
        && slot_at(0).map(|c| c.scenario) == Some(Scenario::SysvHead)
        && slot_at(13).map(|c| c.scenario) == Some(Scenario::NameBad)
}

/// 渲染。
fn report_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.render()
        == "symhash corpus=14 accept=8 reject=6 struct=2 count=1 offset=2 bloom=1 match=14 rate=1000ppm\n";
    let b = r.rate() == 1000 && r.matched() == 14;
    let c = r.green() && r.structure() + r.count() + r.offset() + r.bloom() == 6;
    a && b && c
}

/// 红档。
fn reds_ok() -> bool {
    let red = match run_with(Some((2, HS_EOFF))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = red.matched() == 13 && red.rate() == 929;
    let b = !red.green() && red.gate() == Err(HS_ESTATE);
    let c = run_with(Some((14, HS_EOFF))) == Err(HS_ESLOT);
    let d = run_with(Some((0, 0))) == Err(HS_EVALUE);
    a && b && c && d
}

/// 指纹与文本。
fn hash_ok() -> bool {
    let a = u32_text(0) == "0" && u32_text(929) == "929";
    let b = hex_text(0xfe20) == "0x0000fe20";
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
    let e = l.register(Sample::blank()) == Err(HS_ESEAL);
    let f = l.rejects() == 1 && l.freeze() == Err(HS_ESTATE);
    let g = l.sample(7).map(|s| s.value) == Some(929);
    let h = build_ledger().seal() == root;
    a && b && c && d && e && f && g && h
}

/// 四注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = hash_fn_ok() && syms_ok() && sysv_ok();
    out[1] = sysv_lookup_ok() && gnu_ok() && null_ok();
    out[2] = bucket_ok() && chain_ok() && nchain_ok() && offset_ok();
    out[3] = name_ok()
        && bloom_ok()
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
pub fn run_unx11629_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11629.symhash_corrupt");

    s.add("HS01-ELF 与 GNU 哈希函数", hash_fn_ok(), "static");
    s.add("HS02-符号表名字与偏移", syms_ok(), "static");
    s.add("HS03-SysV 表结构与桶链编码", sysv_ok(), "static");
    s.add("HS04-SysV 命中与未定义", sysv_lookup_ok(), "static");
    s.add("HS05-GNU 布隆与链", gnu_ok(), "static");
    s.add("HS06-桶数非零与桶值上界", bucket_ok(), "static");
    s.add("HS07-链自环与步数上界", chain_ok(), "static");
    s.add("HS08-nchain 与 dynsym 对齐", nchain_ok(), "static");
    s.add("HS09-哈希节偏移越界", offset_ok(), "static");
    s.add("HS10-符号名偏移越界", name_ok(), "static");
    s.add("HS11-布隆参数非法", bloom_ok() && gnu_ok(), "static");
    s.add("HS12-dlsym NULL 与十四槽渲染", null_ok() && corpus_ok() && slots_ok() && report_ok() && reds_ok(), "static");
    s.add("HS13-冻结重放四注入", inject_four() == [true; 4], "static");

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
        assert_eq!(l.register(Sample::blank()), Err(HS_ESEAL));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn injections() {
        assert_eq!(inject_four(), [true; 4]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11629_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }

    #[test]
    fn text_tools() {
        assert_eq!(u32_text(0).as_str(), "0");
        assert_eq!(u32_text(14).as_str(), "14");
        assert_eq!(hex_text(0xfe20).as_str(), "0x0000fe20");
    }

    #[test]
    fn elf_hash_values() {
        assert_eq!(elf_hash(b""), 0);
        assert_eq!(elf_hash(b"a"), 97);
        assert_eq!(elf_hash(b"abc"), 26499);
        assert_eq!(elf_hash(b"malloc"), elf_hash(b"malloc"));
    }

    #[test]
    fn gnu_hash_values() {
        assert_eq!(gnu_hash(b""), 5381);
        assert_eq!(gnu_hash(b"a"), 5381 * 33 + 97);
        assert_ne!(gnu_hash(b"abc"), elf_hash(b"abc"));
        assert!(pow2(8) && !pow2(6));
    }

    #[test]
    fn symtab_basics() {
        let s = syms4();
        assert_eq!(s.len(), 4);
        assert_eq!(s.name_at(0), Some("malloc"));
        assert_eq!(s.off_at(1), Some(8));
        assert_eq!(s.find("calloc"), Some(2));
        assert_eq!(s.find("nope"), None);
        assert_eq!(s.name_at(9), None);
        assert!(s.names_ok());
    }

    #[test]
    fn sysv_encoding() {
        let o = object_for(Scenario::SysvHead);
        assert_eq!(o.sysv.bucket_at(0), Some(1));
        assert_eq!(o.sysv.chain_at(0), Some(2));
        assert_eq!(o.sysv.chain_at(3), Some(0));
        assert_eq!(o.sysv.walk_len(0), 4);
        assert!(o.sysv.buckets_ok() && o.sysv.chains_ok());
    }

    #[test]
    fn sysv_head_hit() {
        let j = probe(&object_for(Scenario::SysvHead), "malloc");
        assert!(j.clean());
        assert_eq!(j.sym(), Some(0));
        assert!(!j.null());
        assert_eq!(j.trace.last(), Some((Stage::Done, true)));
    }

    #[test]
    fn sysv_tail_hit() {
        let j = probe(&object_for(Scenario::SysvTail), "free");
        assert!(j.clean());
        assert_eq!(j.sym(), Some(1));
    }

    #[test]
    fn sysv_undefined_is_clean() {
        let j = probe(&object_for(Scenario::SysvMiss), "strlen");
        assert!(j.clean());
        assert_eq!(j.sym(), None);
        assert!(j.null());
        assert_eq!(j.code(), 0);
    }

    #[test]
    fn gnu_bloom_hit() {
        let o = object_for(Scenario::GnuHit);
        assert!(o.gnu.bloom_maybe(gnu_hash(b"memcpy")));
        let j = probe(&o, "memcpy");
        assert!(j.clean());
        assert_eq!(j.sym(), Some(1));
    }

    #[test]
    fn gnu_miss_clean() {
        let j = probe(&object_for(Scenario::GnuMiss), "nothere");
        assert!(j.clean());
        assert_eq!(j.sym(), None);
    }

    #[test]
    fn empty_table_clean() {
        let j = probe(&object_for(Scenario::EmptyTable), "malloc");
        assert!(j.clean());
        assert_eq!(j.sym(), None);
        assert!(j.null());
    }

    #[test]
    fn long_name_hit() {
        let j = probe(&object_for(Scenario::LongName), "pthread_create");
        assert!(j.clean());
        assert_eq!(j.sym(), Some(3));
    }

    #[test]
    fn zero_bucket_reject() {
        let j = probe(&object_for(Scenario::ZeroBucket), "malloc");
        assert_eq!(j.code(), HS_EBUCKET);
        assert!(j.null());
        assert_eq!(j.trace.last(), Some((Stage::Bucket, false)));
    }

    #[test]
    fn chain_loop_reject() {
        let o = object_for(Scenario::ChainLoop);
        assert!(!o.sysv.chains_ok());
        let j = probe(&o, "malloc");
        assert_eq!(j.code(), HS_ECHAIN);
        assert!(j.null());
    }

    #[test]
    fn nchain_reject() {
        let j = probe(&object_for(Scenario::NchainBad), "malloc");
        assert_eq!(j.code(), HS_ENCHAIN);
        assert_eq!(j.trace.last(), Some((Stage::Count, false)));
    }

    #[test]
    fn offset_reject() {
        let j = probe(&object_for(Scenario::OffBad), "malloc");
        assert_eq!(j.code(), HS_EOFF);
        assert_eq!(j.trace.last(), Some((Stage::Bounds, false)));
    }

    #[test]
    fn bloom_reject() {
        let j = probe(&object_for(Scenario::BloomBad), "memcpy");
        assert_eq!(j.code(), HS_EBLOOM);
        assert_eq!(j.trace.last(), Some((Stage::Bloom, false)));
    }

    #[test]
    fn name_reject() {
        let j = probe(&object_for(Scenario::NameBad), "malloc");
        assert_eq!(j.code(), HS_ENAME);
        assert_eq!(j.trace.last(), Some((Stage::Names, false)));
    }

    #[test]
    fn dlsym_texts() {
        let o = object_for(Scenario::SysvHead);
        assert_eq!(o.dlsym_text("malloc").as_str(), "dlsym ok malloc -> malloc");
        assert_eq!(o.dlsym_text("absent").as_str(), "undefined symbol: absent");
        assert!(o.dlsym_text("absent").starts_with("undefined symbol: "));
    }

    #[test]
    fn corrupt_texts() {
        let o = object_for(Scenario::OffBad);
        let t = o.dlsym_text("malloc");
        assert!(t.starts_with("./obj: hash section offset 900"));
        let b = object_for(Scenario::ChainLoop);
        assert!(b.dlsym_text("malloc").contains("does not terminate"));
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
            "symhash corpus=14 accept=8 reject=6 struct=2 count=1 offset=2 bloom=1 match=14 rate=1000ppm\n"
        );
    }

    #[test]
    fn red_rate() {
        let r = match run_with(Some((1, HS_EOFF))) {
            Ok(v) => v,
            Err(_) => Report::new(),
        };
        assert_eq!(r.rate(), 929);
        assert_eq!(r.gate(), Err(HS_ESTATE));
    }

    #[test]
    fn cover_prefix() {
        assert_eq!(cover(0), Ok(()));
        assert_eq!(cover(14), Ok(()));
        assert_eq!(cover(15), Err(HS_ESLOT));
    }

    #[test]
    fn error_codes_distinct() {
        let codes = [
            HS_EBUCKET,
            HS_ECHAIN,
            HS_ENCHAIN,
            HS_EOFF,
            HS_EBLOOM,
            HS_ENAME,
            HS_ESTATE,
            HS_EVER,
            HS_EBAL,
            HS_ESLOT,
            HS_EVALUE,
            HS_ESEAL,
            HS_ENOBS,
        ];
        let mut i = 0usize;
        while i < codes.len() {
            assert_eq!(codes[i], SEG_BASE + 1 + i as u32);
            i += 1;
        }
    }
}