//! UNX-F11245 ld.so.cache 格式消费（0x1f2d0 段）
//!
//! 模型：ld.so.cache 是一段自描述二进制——16 字节文件头（magic/nlibs/
//! len_strings/flags）+ nlibs 条 24 字节条目（flags/key 偏移/value 偏移/
//! osversion/hwcap）+ 字符串区（键值成对、NUL 分隔）。解析器逐字段消费并
//! 把字符串搬进定容缓冲，之后按 key 精确查找返回 value。
//!
//! 规则面：magic 必须匹配、nlibs ≤ 8、len_strings ≤ 96、条目区与字符串区都
//! 必须落在文件长度内、头 flags 与条目 flags 只允许已知位、key/value 偏移必须
//! 小于 len_strings 且其后必须有 NUL、条目必须按 key 严格升序（相等即重复键）。
//! 命中返回路径，未命中返回 ENOENT，两者都不改动解析结果。
//!
//! 账本：每样本一条 slot（键名 + 结果类别 + 命中路径），按命中/未命中/坏头/
//! 坏条目四桶计数。fold 卷全账；replay 重新解析并用同一 key 复算；freeze 后
//! 一切登记与观测都拒。
//!
//! 拒止面：坏魔数、截断、条目过多、字符串区过大、头/条目标志位未知、偏移越
//! 界、字符串未终止、乱序键、重复键、样本号越界、跳号/重号登记、冻结后写入。

use crate::alloc::format;
use crate::checks::CheckSet;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f2d0;

/// 样本号越界
pub const CE_EID: u32 = SEG_BASE + 0x01;
/// 登记序违例
pub const CE_EORDER: u32 = SEG_BASE + 0x02;
/// 重号登记 / 重号观测
pub const CE_EDUP: u32 = SEG_BASE + 0x03;
/// 冻结后写入
pub const CE_EFROZEN: u32 = SEG_BASE + 0x04;
/// 未观测即重放
pub const CE_ENOBS: u32 = SEG_BASE + 0x05;
/// 重放不一致
pub const CE_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 分桶不平衡
pub const CE_EBAL: u32 = SEG_BASE + 0x07;
/// 空账重放
pub const CE_EEMPTY: u32 = SEG_BASE + 0x08;
/// 坏魔数
pub const CE_EMAGIC: u32 = SEG_BASE + 0x09;
/// 文件头或条目区截断
pub const CE_ETRUNC: u32 = SEG_BASE + 0x0a;
/// 条目过多 / 字符串区过大
pub const CE_EBIG: u32 = SEG_BASE + 0x0b;
/// 标志位未知
pub const CE_EFLAGS: u32 = SEG_BASE + 0x0c;
/// key/value 偏移越界
pub const CE_EOFF: u32 = SEG_BASE + 0x0d;
/// 字符串未以 NUL 结束
pub const CE_ENUL: u32 = SEG_BASE + 0x0e;
/// 条目乱序 / 重复键
pub const CE_ESORT: u32 = SEG_BASE + 0x0f;
/// 查找未命中
pub const CE_ENOENT: u32 = SEG_BASE + 0x10;

// ---------------------------------------------------------------- 域常量
/// 镜像缓冲上限
pub const IMG_CAP: usize = 256;
/// 字符串区上限
pub const STR_CAP: usize = 96;
/// 条目上限
pub const MAX_ENTRIES: usize = 8;
/// 条目尺寸
pub const ENTRY_SIZE: usize = 24;
/// 文件头尺寸
pub const HDR_SIZE: usize = 16;
/// 缓存魔数
pub const MAGIC: u32 = 0x0bad_c0de;
/// 头标志：新格式
pub const FLAG_NEW: u8 = 0x01;
/// 头标志：带 hwcap 扩展
pub const FLAG_HWCAP: u8 = 0x02;
/// 头标志掩码
pub const FLAG_MASK: u8 = FLAG_NEW | FLAG_HWCAP;
/// 条目标志：ELF
pub const EF_ELF: u32 = 0x0000_0001;
/// 条目标志：hwcap
pub const EF_HWCAP: u32 = 0x0000_0002;
/// 条目标志掩码
pub const EF_MASK: u32 = EF_ELF | EF_HWCAP;
/// 命中路径缓冲
pub const PATH_CAP: usize = 32;
/// 结果：命中
pub const OC_HIT: u8 = 0;
/// 结果：未命中
pub const OC_MISS: u8 = 1;
/// 结果：错误起始序号
pub const OC_ERR: u8 = 2;
/// ENOENT
pub const ENOENT: u32 = 2;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

// ---------------------------------------------------------------- 字节镜像
/// 定容字节镜像（cache 文件的原始字节）。
#[derive(Clone, Copy, Debug)]
pub struct Image {
    bytes: [u8; IMG_CAP],
    len: usize,
}

impl Image {
    /// 空镜像。
    pub const fn blank() -> Image {
        Image { bytes: [0; IMG_CAP], len: 0 }
    }

    /// 当前长度。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 设长度（只能 ≤ IMG_CAP）。
    pub fn set_len(&mut self, n: usize) -> bool {
        if n > IMG_CAP {
            return false;
        }
        self.len = n;
        true
    }

    /// 取一字节。
    pub fn byte(&self, off: usize) -> Option<u8> {
        if off >= self.len {
            return None;
        }
        self.bytes.get(off).copied()
    }

    /// 取一段。
    pub fn slice(&self, off: usize, n: usize) -> Option<&[u8]> {
        if off.checked_add(n)? > self.len {
            return None;
        }
        self.bytes.get(off..off + n)
    }

    /// 写一字节。
    pub fn put_u8(&mut self, off: usize, v: u8) -> bool {
        match self.bytes.get_mut(off) {
            Some(d) => {
                *d = v;
                if off >= self.len {
                    self.len = off + 1;
                }
                true
            }
            None => false,
        }
    }

    /// 写小端 u32。
    pub fn put_u32(&mut self, off: usize, v: u32) -> bool {
        let b = v.to_le_bytes();
        let mut i = 0usize;
        while i < 4 {
            match (self.bytes.get_mut(off + i), b.get(i)) {
                (Some(d), Some(s)) => *d = *s,
                _ => return false,
            }
            i = i.wrapping_add(1);
        }
        if off + 4 > self.len {
            self.len = off + 4;
        }
        true
    }

    /// 写小端 u64。
    pub fn put_u64(&mut self, off: usize, v: u64) -> bool {
        let b = v.to_le_bytes();
        let mut i = 0usize;
        while i < 8 {
            match (self.bytes.get_mut(off + i), b.get(i)) {
                (Some(d), Some(s)) => *d = *s,
                _ => return false,
            }
            i = i.wrapping_add(1);
        }
        if off + 8 > self.len {
            self.len = off + 8;
        }
        true
    }

    /// 写一段字节。
    pub fn put_bytes(&mut self, off: usize, src: &[u8]) -> bool {
        let mut i = 0usize;
        while i < src.len() {
            match (self.bytes.get_mut(off + i), src.get(i)) {
                (Some(d), Some(s)) => *d = *s,
                _ => return false,
            }
            i = i.wrapping_add(1);
        }
        if off + src.len() > self.len {
            self.len = off + src.len();
        }
        true
    }

    /// 读小端 u32。
    pub fn get_u32(&self, off: usize) -> Option<u32> {
        let s = self.slice(off, 4)?;
        Some(u32::from_le_bytes([*s.first()?, *s.get(1)?, *s.get(2)?, *s.get(3)?]))
    }

    /// 读小端 u64。
    pub fn get_u64(&self, off: usize) -> Option<u64> {
        let s = self.slice(off, 8)?;
        let mut b = [0u8; 8];
        let mut i = 0usize;
        while i < 8 {
            b[i] = *s.get(i)?;
            i = i.wrapping_add(1);
        }
        Some(u64::from_le_bytes(b))
    }
}

// ---------------------------------------------------------------- 解析
/// 一条 cache 条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub flags: u32,
    pub key_off: u32,
    pub value_off: u32,
    pub osversion: u32,
    pub hwcap: u64,
}

/// 解析后的 cache：条目表 + 字符串区副本。
#[derive(Clone, Copy, Debug)]
pub struct Parsed {
    entries: [Option<Entry>; MAX_ENTRIES],
    pub n: usize,
    pub len_strings: u32,
    pub flags: u8,
    strings: [u8; STR_CAP],
    pub strings_len: usize,
}

/// 在镜像 [start, limit) 内读一个 NUL 结尾串（不含 NUL）。
pub fn cstr(img: &Image, start: usize, limit: usize) -> Option<&[u8]> {
    if start >= limit || limit > img.len() {
        return None;
    }
    let mut i = start;
    while i < limit {
        match img.byte(i) {
            Some(0) => return img.slice(start, i - start),
            Some(_) => {}
            None => return None,
        }
        i = i.wrapping_add(1);
    }
    None
}

/// 在已拷入的字符串缓冲里读一个 NUL 结尾串。
pub fn cstr_buf(buf: &[u8], len: usize, start: usize) -> Option<&[u8]> {
    if start >= len {
        return None;
    }
    let mut i = start;
    while i < len {
        match buf.get(i) {
            Some(0) => return buf.get(start..i),
            Some(_) => {}
            None => return None,
        }
        i = i.wrapping_add(1);
    }
    None
}

/// 读一条条目。
fn read_entry(img: &Image, off: usize) -> Result<Entry, u32> {
    let flags = match img.get_u32(off) {
        Some(v) => v,
        None => return Err(CE_ETRUNC),
    };
    let key_off = match img.get_u32(off + 4) {
        Some(v) => v,
        None => return Err(CE_ETRUNC),
    };
    let value_off = match img.get_u32(off + 8) {
        Some(v) => v,
        None => return Err(CE_ETRUNC),
    };
    let osversion = match img.get_u32(off + 12) {
        Some(v) => v,
        None => return Err(CE_ETRUNC),
    };
    let hwcap = match img.get_u64(off + 16) {
        Some(v) => v,
        None => return Err(CE_ETRUNC),
    };
    Ok(Entry { flags, key_off, value_off, osversion, hwcap })
}

/// 解析整个 cache 镜像。
pub fn parse(img: &Image) -> Result<Parsed, u32> {
    if img.len() < HDR_SIZE {
        return Err(CE_ETRUNC);
    }
    let magic = match img.get_u32(0) {
        Some(v) => v,
        None => return Err(CE_ETRUNC),
    };
    if magic != MAGIC {
        return Err(CE_EMAGIC);
    }
    let nlibs = match img.get_u32(4) {
        Some(v) => v as usize,
        None => return Err(CE_ETRUNC),
    };
    if nlibs > MAX_ENTRIES {
        return Err(CE_EBIG);
    }
    let len_strings = match img.get_u32(8) {
        Some(v) => v,
        None => return Err(CE_ETRUNC),
    };
    if len_strings as usize > STR_CAP {
        return Err(CE_EBIG);
    }
    let flags = match img.byte(12) {
        Some(v) => v,
        None => return Err(CE_ETRUNC),
    };
    if flags & !FLAG_MASK != 0 {
        return Err(CE_EFLAGS);
    }
    let base = HDR_SIZE + nlibs * ENTRY_SIZE;
    let str_end = base + len_strings as usize;
    if str_end > img.len() {
        return Err(CE_ETRUNC);
    }
    let mut p = Parsed {
        entries: [None; MAX_ENTRIES],
        n: 0,
        len_strings,
        flags,
        strings: [0; STR_CAP],
        strings_len: 0,
    };
    let src = match img.slice(base, len_strings as usize) {
        Some(v) => v,
        None => return Err(CE_ETRUNC),
    };
    let mut k = 0usize;
    while k < len_strings as usize {
        if let (Some(dst), Some(s)) = (p.strings.get_mut(k), src.get(k)) {
            *dst = *s;
        }
        k = k.wrapping_add(1);
    }
    p.strings_len = len_strings as usize;
    let mut prev: Option<&[u8]> = None;
    let mut i = 0usize;
    while i < nlibs {
        let e = read_entry(img, HDR_SIZE + i * ENTRY_SIZE)?;
        if e.flags & !EF_MASK != 0 {
            return Err(CE_EFLAGS);
        }
        if e.key_off >= len_strings || e.value_off >= len_strings {
            return Err(CE_EOFF);
        }
        let key = match cstr(img, base + e.key_off as usize, str_end) {
            Some(v) => v,
            None => return Err(CE_ENUL),
        };
        if cstr(img, base + e.value_off as usize, str_end).is_none() {
            return Err(CE_ENUL);
        }
        if let Some(pv) = prev {
            if pv >= key {
                return Err(CE_ESORT);
            }
        }
        prev = Some(key);
        if let Some(slot) = p.entries.get_mut(i) {
            *slot = Some(e);
        }
        i = i.wrapping_add(1);
    }
    p.n = nlibs;
    Ok(p)
}

impl Parsed {
    /// 第 i 条的键串。
    pub fn key(&self, i: usize) -> Option<&[u8]> {
        let e = match self.entries.get(i) {
            Some(Some(v)) => *v,
            _ => return None,
        };
        cstr_buf(&self.strings, self.strings_len, e.key_off as usize)
    }

    /// 第 i 条的值串。
    pub fn value(&self, i: usize) -> Option<&[u8]> {
        let e = match self.entries.get(i) {
            Some(Some(v)) => *v,
            _ => return None,
        };
        cstr_buf(&self.strings, self.strings_len, e.value_off as usize)
    }

    /// 命中返回路径，未命中返回 ENOENT。
    pub fn lookup(&self, key: &[u8]) -> Result<&[u8], u32> {
        let mut i = 0usize;
        while i < self.n {
            if let Some(k) = self.key(i) {
                if k == key {
                    return match self.value(i) {
                        Some(v) => Ok(v),
                        None => Err(CE_ENOENT),
                    };
                }
            }
            i = i.wrapping_add(1);
        }
        Err(CE_ENOENT)
    }
}

/// 按「键值对 + 头标志」构造一个合法 cache 镜像。
pub fn build_image(pairs: &[(&str, &str)], flags: u8) -> Image {
    let mut img = Image::blank();
    let n = if pairs.len() > MAX_ENTRIES { MAX_ENTRIES } else { pairs.len() };
    let base = HDR_SIZE + n * ENTRY_SIZE;
    let mut off = 0u32;
    let mut i = 0usize;
    while i < n {
        let (k, v) = match pairs.get(i) {
            Some(p) => *p,
            None => break,
        };
        let _ = img.put_u32(HDR_SIZE + i * ENTRY_SIZE, EF_ELF);
        let _ = img.put_u32(HDR_SIZE + i * ENTRY_SIZE + 4, off);
        off = off.wrapping_add(k.len() as u32 + 1);
        let _ = img.put_u32(HDR_SIZE + i * ENTRY_SIZE + 8, off);
        off = off.wrapping_add(v.len() as u32 + 1);
        let _ = img.put_u32(HDR_SIZE + i * ENTRY_SIZE + 12, 0);
        let _ = img.put_u64(HDR_SIZE + i * ENTRY_SIZE + 16, 0);
        i = i.wrapping_add(1);
    }
    let _ = img.put_u32(0, MAGIC);
    let _ = img.put_u32(4, n as u32);
    let _ = img.put_u32(8, off);
    let _ = img.put_u8(12, flags);
    let mut at = base;
    let mut j = 0usize;
    while j < n {
        let (k, v) = match pairs.get(j) {
            Some(p) => *p,
            None => break,
        };
        let _ = img.put_bytes(at, k.as_bytes());
        at += k.len();
        let _ = img.put_u8(at, 0);
        at += 1;
        let _ = img.put_bytes(at, v.as_bytes());
        at += v.len();
        let _ = img.put_u8(at, 0);
        at += 1;
        j = j.wrapping_add(1);
    }
    let total = base + off as usize;
    if img.len() < total {
        let _ = img.set_len(total);
    }
    img
}

/// 12 个样本镜像：3 命中、2 未命中、7 结构坏。
pub fn sample_image(id: usize) -> Image {
    match id {
        0 => build_image(&[("libc.so.6", "/lib/libc.so.6")], FLAG_NEW),
        1 | 2 => build_image(
            &[("libm.so.6", "/lib/libm.so.6"), ("libz.so.1", "/lib/libz.so.1")],
            FLAG_NEW,
        ),
        3 => {
            let mut img = build_image(&[("libc.so.6", "/lib/libc.so.6")], FLAG_NEW);
            let _ = img.put_u32(0, 0xdead_beef);
            img
        }
        4 => {
            let mut img = build_image(
                &[("a.so", "/a"), ("b.so", "/b"), ("c.so", "/c")],
                FLAG_NEW,
            );
            let _ = img.set_len(HDR_SIZE + ENTRY_SIZE);
            img
        }
        5 => build_image(&[("z.so.1", "/z"), ("a.so.1", "/a")], FLAG_NEW),
        6 => {
            let mut img = build_image(&[("libc.so.6", "/lib/libc.so.6")], FLAG_NEW);
            let _ = img.put_u32(HDR_SIZE + 4, 0x200);
            img
        }
        7 => {
            let mut img = build_image(&[("libc.so.6", "/lib/libc.so.6")], FLAG_NEW);
            let end = img.len();
            let _ = img.put_u8(end - 1, b'X');
            img
        }
        8 => {
            let mut img = build_image(&[("libc.so.6", "/opt/libc.so.6")], FLAG_HWCAP);
            let _ = img.put_u64(HDR_SIZE + 16, EF_HWCAP as u64);
            img
        }
        9 => build_image(&[], FLAG_NEW),
        10 => {
            let mut img = build_image(&[("libc.so.6", "/lib/libc.so.6")], FLAG_NEW);
            let _ = img.put_u32(HDR_SIZE, 0x8000_0000);
            img
        }
        _ => build_image(&[("libc.so.6", "/a/libc.so.6"), ("libc.so.6", "/b/libc.so.6")], FLAG_NEW),
    }
}

/// 样本查询键。
pub fn query_of(id: usize) -> &'static str {
    match id {
        2 => "libfoo.so.1",
        1 => "libz.so.1",
        _ => "libc.so.6",
    }
}

/// 跑一次样本：返回（结果类别，命中路径，路径长）。
pub fn evaluate(id: usize) -> (u8, [u8; PATH_CAP], usize) {
    let img = sample_image(id);
    let p = match parse(&img) {
        Ok(v) => v,
        Err(code) => return ((code - SEG_BASE) as u8 + 1, [0; PATH_CAP], 0),
    };
    match p.lookup(query_of(id).as_bytes()) {
        Ok(path) => {
            let mut out = [0u8; PATH_CAP];
            let mut n = 0usize;
            while n < PATH_CAP && n < path.len() {
                if let (Some(d), Some(s)) = (out.get_mut(n), path.get(n)) {
                    *d = *s;
                }
                n = n.wrapping_add(1);
            }
            (OC_HIT, out, n)
        }
        Err(_) => (OC_MISS, [0; PATH_CAP], 0),
    }
}

// ---------------------------------------------------------------- 账本
/// 样本总数。
pub const SAMPLES: usize = 12;

/// 一条查找账。
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    pub id: usize,
    pub verdict: u8,
    pub path: [u8; PATH_CAP],
    pub path_len: usize,
    pub observed: bool,
}

impl Slot {
    /// 空账位。
    pub const fn blank() -> Slot {
        Slot { id: 0, verdict: 0, path: [0; PATH_CAP], path_len: 0, observed: false }
    }

    /// 是否命中。
    pub fn hit(&self) -> bool {
        self.verdict == OC_HIT
    }
}

/// 命中/未命中两路径账本。
#[derive(Clone, Debug)]
pub struct CacheLedger {
    slots: [Option<Slot>; SAMPLES],
    n: usize,
    hits: u32,
    misses: u32,
    errors: u32,
    observed: u32,
    accepts: u64,
    rejects: u64,
    root: Option<u64>,
    frozen: bool,
}

impl CacheLedger {
    /// 空账。
    pub const fn new() -> CacheLedger {
        CacheLedger {
            slots: [None; SAMPLES],
            n: 0,
            hits: 0,
            misses: 0,
            errors: 0,
            observed: 0,
            accepts: 0,
            rejects: 0,
            root: None,
            frozen: false,
        }
    }

    /// 记一次拒止。
    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 已登样本数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 命中数。
    pub fn hits(&self) -> u32 {
        self.hits
    }

    /// 未命中数。
    pub fn misses(&self) -> u32 {
        self.misses
    }

    /// 结构坏样本数。
    pub fn errors(&self) -> u32 {
        self.errors
    }

    /// 已观测数。
    pub fn observed(&self) -> u32 {
        self.observed
    }

    /// 取一条账。
    pub fn slot(&self, i: usize) -> Option<Slot> {
        match self.slots.get(i) {
            Some(v) => *v,
            None => None,
        }
    }

    /// 定锚指纹。
    pub fn root_value(&self) -> Option<u64> {
        self.root
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 登记一个样本并跑一次查找。
    pub fn register(&mut self, id: usize) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.fail(CE_EFROZEN));
        }
        if id >= SAMPLES {
            return Err(self.fail(CE_EID));
        }
        if id < self.n {
            return Err(self.fail(CE_EDUP));
        }
        if id > self.n {
            return Err(self.fail(CE_EORDER));
        }
        let (verdict, path, path_len) = evaluate(id);
        let slot = Slot { id, verdict, path, path_len, observed: false };
        match verdict {
            OC_HIT => self.hits = self.hits.wrapping_add(1),
            OC_MISS => self.misses = self.misses.wrapping_add(1),
            _ => self.errors = self.errors.wrapping_add(1),
        }
        let at = self.n;
        if let Some(cell) = self.slots.get_mut(at) {
            *cell = Some(slot);
        }
        self.n = self.n.wrapping_add(1);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 观测一条账。
    pub fn observe(&mut self, id: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(CE_EFROZEN));
        }
        if id >= self.n {
            return Err(self.fail(CE_EORDER));
        }
        let cell = match self.slots.get(id) {
            Some(v) => *v,
            None => None,
        };
        match cell {
            Some(mut s) => {
                if s.observed {
                    return Err(self.fail(CE_EDUP));
                }
                s.observed = true;
                if let Some(dst) = self.slots.get_mut(id) {
                    *dst = Some(s);
                }
                self.observed = self.observed.wrapping_add(1);
                Ok(())
            }
            None => Err(self.fail(CE_EORDER)),
        }
    }

    /// 全账指纹：样本号 + 结果 + 路径字节 + 三桶计数。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.n {
            if let Some(Some(s)) = self.slots.get(i) {
                h = (h ^ s.id as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ s.verdict as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ s.path_len as u64).wrapping_mul(FNV_PRIME);
                let mut k = 0usize;
                while k < PATH_CAP {
                    let b = match s.path.get(k) {
                        Some(v) => *v as u64,
                        None => 0,
                    };
                    h = (h ^ b).wrapping_mul(FNV_PRIME);
                    k = k.wrapping_add(1);
                }
                h = (h ^ s.observed as u64).wrapping_mul(FNV_PRIME);
            }
            i = i.wrapping_add(1);
        }
        h = (h ^ self.n as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.hits as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.misses as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.errors as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.accepts).wrapping_mul(FNV_PRIME);
        h = (h ^ self.rejects).wrapping_mul(FNV_PRIME);
        h
    }
}

impl CacheLedger {
    /// 重放：逐条重跑解析+查找，路径逐字节比对，三桶合计等于样本数。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if self.n == 0 {
            return Err(self.fail(CE_EEMPTY));
        }
        if self.observed as usize != self.n {
            return Err(self.fail(CE_ENOBS));
        }
        let mut i = 0usize;
        while i < self.n {
            let s = match self.slots.get(i).copied() {
                Some(Some(v)) => v,
                _ => return Err(self.fail(CE_EORDER)),
            };
            let (verdict, path, path_len) = evaluate(s.id);
            if verdict != s.verdict || path_len != s.path_len {
                return Err(self.fail(CE_EMISMATCH));
            }
            let mut k = 0usize;
            while k < path_len {
                if path.get(k) != s.path.get(k) {
                    return Err(self.fail(CE_EMISMATCH));
                }
                k = k.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        let total = self.hits.wrapping_add(self.misses).wrapping_add(self.errors);
        if total != self.n as u32 {
            return Err(self.fail(CE_EBAL));
        }
        let v = self.fold();
        match self.root {
            Some(r) if r != v => Err(self.fail(CE_EMISMATCH)),
            _ => Ok(v),
        }
    }

    /// 先重放再封账；二次冻结拒。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.fail(CE_EFROZEN));
        }
        let v = self.replay()?;
        self.root = Some(v);
        self.frozen = true;
        Ok(v)
    }
}

/// 建全账：12 样本登记 → 全观测 → 封账。
pub fn build_ledger() -> CacheLedger {
    let mut l = CacheLedger::new();
    let mut i = 0usize;
    while i < SAMPLES {
        let _ = l.register(i);
        i = i.wrapping_add(1);
    }
    let mut k = 0usize;
    while k < l.len() {
        let _ = l.observe(k);
        k = k.wrapping_add(1);
    }
    let _ = l.freeze();
    l
}

/// 四路独立拒止：登记序 / 重号 / 越界样本号 / 冻结后登记。
pub fn inject_four() -> [bool; 4] {
    let mut a = CacheLedger::new();
    let order = a.register(1).err() == Some(CE_EORDER) && a.len() == 0;

    let mut b = CacheLedger::new();
    let first = b.register(0).is_ok();
    let dup = b.register(0).err() == Some(CE_EDUP) && b.len() == 1;

    let mut c = CacheLedger::new();
    let range = c.register(SAMPLES).err() == Some(CE_EID) && c.len() == 0;

    let mut d = build_ledger();
    let frozen = d.register(0).err() == Some(CE_EFROZEN) && d.is_frozen();

    [order, first && dup, range, frozen]
}

// ---------------------------------------------------------------- 自检
/// 判据：UNX-F11245 cache 文件头/条目格式 + 命中与未命中两路径。
pub fn run_unx11245_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11245");
    let img0 = sample_image(0);
    let p0 = parse(&img0);

    // C1 文件头三字段
    let mut hdr = false;
    if let Ok(p) = p0 {
        hdr = p.len_strings as usize == "libc.so.6".len() + "/lib/libc.so.6".len() + 2
            && p.flags == FLAG_NEW
            && p.n == 1;
    }
    s.add_dyn("C1-文件头解析", hdr, &format!("parsed={}", p0.is_ok()));

    // C2 条目偏移解析
    let mut entry_ok = false;
    if let Ok(p) = p0 {
        entry_ok = p.key(0) == Some(&b"libc.so.6"[..])
            && p.value(0) == Some(&b"/lib/libc.so.6"[..]);
    }
    s.add("C2-条目偏移解析", entry_ok, "第一条 key/value 偏移取回原串");

    // C3 命中路径
    let (v0, path0, n0) = evaluate(0);
    let hit_path = match path0.get(..n0) {
        Some(b) => core::str::from_utf8(b).unwrap_or(""),
        None => "",
    };
    s.add_dyn(
        "C3-命中路径",
        v0 == OC_HIT && hit_path == "/lib/libc.so.6",
        &format!("verdict={} path={}", v0, hit_path),
    );

    // C4 两条目中第二条命中
    let (v1, path1, n1) = evaluate(1);
    let mut ok1 = false;
    if let Ok(t) = core::str::from_utf8(&path1[..n1]) {
        ok1 = v1 == OC_HIT && t == "/lib/libz.so.1";
    }
    s.add_dyn("C4-多条目命中", ok1, &format!("verdict={} len={}", v1, n1));

    // C5 未命中路径
    let (v2, _, _) = evaluate(2);
    s.add(
        "C5-未命中路径",
        v2 == OC_MISS && parse(&sample_image(2)).is_ok(),
        "键不存在返回未命中，解析结果保持有效",
    );

    // C6 坏魔数
    s.add(
        "C6-坏魔数拒止",
        parse(&sample_image(3)).err() == Some(CE_EMAGIC),
        "magic 不匹配报 EMAGIC",
    );

    // C7 截断拒止
    s.add(
        "C7-截断拒止",
        parse(&sample_image(4)).err() == Some(CE_ETRUNC),
        "条目区加字符串区越过文件长度报 ETRUNC",
    );

    // C8 乱序键拒止
    s.add(
        "C8-乱序键拒止",
        parse(&sample_image(5)).err() == Some(CE_ESORT),
        "z 在 a 之前报 ESORT",
    );

    // C9 偏移越界
    s.add(
        "C9-偏移越界拒止",
        parse(&sample_image(6)).err() == Some(CE_EOFF),
        "key 偏移 0x200 超过字符串区报 EOFF",
    );

    // C10 未终止串
    s.add(
        "C10-未终止串拒止",
        parse(&sample_image(7)).err() == Some(CE_ENUL),
        "末尾 NUL 被覆写后报 ENUL",
    );

    // C11 重复键与非法条目标志
    s.add(
        "C11-重复键与条目标志",
        parse(&sample_image(11)).err() == Some(CE_ESORT)
            && parse(&sample_image(10)).err() == Some(CE_EFLAGS),
        "同名键不满足严格升序，未知标志位报 EFLAGS",
    );

    // C12 三桶平衡 + 重放
    let l = build_ledger();
    let mut r = l.clone();
    let rp = r.replay();
    s.add_dyn(
        "C12-三桶平衡与重放",
        l.hits() == 3
            && l.misses() == 2
            && l.errors() == 7
            && rp.is_ok()
            && l.hits() + l.misses() + l.errors() == SAMPLES as u32,
        &format!("hits={} miss={} err={}", l.hits(), l.misses(), l.errors()),
    );

    // C13 拒止与冻结
    let inj = inject_four();
    s.add(
        "C13-拒止四路与冻结",
        inj[0] && inj[1] && inj[2] && inj[3] && l.is_frozen(),
        "登记序/重号/越界/冻结后写入皆拒",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11245_header_and_entries() {
        match parse(&sample_image(0)) {
            Ok(p) => {
                assert_eq!(p.n, 1);
                assert_eq!(p.len_strings as usize, 9 + 14 + 2);
                assert_eq!(p.key(0), Some(&b"libc.so.6"[..]));
                assert_eq!(p.value(0), Some(&b"/lib/libc.so.6"[..]));
                assert_eq!(p.lookup(b"libc.so.6"), Ok(&b"/lib/libc.so.6"[..]));
                assert_eq!(p.lookup(b"nope.so"), Err(CE_ENOENT));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn f11245_structure_gates() {
        assert_eq!(parse(&sample_image(3)).err(), Some(CE_EMAGIC));
        assert_eq!(parse(&sample_image(4)).err(), Some(CE_ETRUNC));
        assert_eq!(parse(&sample_image(5)).err(), Some(CE_ESORT));
        assert_eq!(parse(&sample_image(6)).err(), Some(CE_EOFF));
        assert_eq!(parse(&sample_image(7)).err(), Some(CE_ENUL));
        assert_eq!(parse(&sample_image(10)).err(), Some(CE_EFLAGS));
        assert_eq!(parse(&sample_image(11)).err(), Some(CE_ESORT));
    }

    #[test]
    fn f11245_hit_miss_paths() {
        let (v0, p0, n0) = evaluate(0);
        assert_eq!(v0, OC_HIT);
        assert_eq!(p0.get(..n0).and_then(|b| core::str::from_utf8(b).ok()), Some("/lib/libc.so.6"));
        let (v2, _, n2) = evaluate(2);
        assert_eq!(v2, OC_MISS);
        assert_eq!(n2, 0);
        assert_eq!(evaluate(9).0, OC_MISS);
        assert_eq!(evaluate(8).0, OC_HIT);
    }

    #[test]
    fn f11245_ledger_replay() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.hits(), 3);
        assert_eq!(l.misses(), 2);
        assert_eq!(l.errors(), 7);
        let root = l.root_value();
        assert!(root.is_some());
        let mut f = l.clone();
        assert_eq!(f.replay().ok(), root);
        assert_eq!(f.freeze().err(), Some(CE_EFROZEN));
    }

    #[test]
    fn f11245_checks_green() {
        let cs = run_unx11245_checks();
        assert_eq!(cs.len(), 13);
        assert!(cs.all_passed());
    }
}
