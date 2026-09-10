//! TRINITY-500 · AI-03 共享卷文件系统域（部分）：FAT32 读写驱动（F051~F073）
//!
//! 依赖 VARIX-500 AI-06 的块设备层与命令层；本文件只做**卷结构**（BPB/FAT/目录项），
//! 不做寄存器编程。与 Windows 互读写是第一目标：字节序、时间戳、LFN 全部按规范来。

// ---------------------------------------------------------------------------
// F051 FAT32 只读驱动 — BPB 解析与簇链
// ---------------------------------------------------------------------------

pub const DIR_ENTRY_SIZE: usize = 32;
pub const EOC_MIN: u32 = 0x0FFF_FFF8;
pub const BAD_CLUSTER: u32 = 0x0FFF_FFF7;
pub const FREE_CLUSTER: u32 = 0x0000_0000;

/// FAT32 引导扇区里我们真正用到的字段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bpb {
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub reserved_sectors: u16,
    pub num_fats: u8,
    pub fat_size_sectors: u32,
    pub root_cluster: u32,
    pub total_sectors: u32,
}

impl Bpb {
    /// 每簇字节数（用于偏移换算）。
    pub fn cluster_bytes(&self) -> u64 {
        self.bytes_per_sector as u64 * self.sectors_per_cluster as u64
    }

    /// 数据区起始扇区。
    pub fn first_data_sector(&self) -> u64 {
        self.reserved_sectors as u64 + self.num_fats as u64 * self.fat_size_sectors as u64
    }

    /// FAT 可容纳的最大合法簇号。
    pub fn max_cluster(&self) -> u32 {
        (self.fat_size_sectors as u64 * self.bytes_per_sector as u64 / 4) as u32
    }
}

fn u16_at(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}

fn u32_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

/// 解析 FAT32 引导扇区（512 字节）。签名不对或字段非法一律拒绝，不猜。
pub fn parse_bpb(boot: &[u8]) -> Option<Bpb> {
    if boot.len() < 512 {
        return None;
    }
    if boot[510] != 0x55 || boot[511] != 0xAA {
        return None;
    }
    let bytes_per_sector = u16_at(boot, 11);
    let sectors_per_cluster = boot[13];
    let reserved = u16_at(boot, 14);
    let num_fats = boot[16];
    let fat_size = u32_at(boot, 36);
    let root_cluster = u32_at(boot, 44);
    let total = u32_at(boot, 32);
    if !matches!(bytes_per_sector, 512 | 1024 | 2048 | 4096) {
        return None;
    }
    if sectors_per_cluster == 0 || !sectors_per_cluster.is_power_of_two() {
        return None;
    }
    if num_fats == 0 || fat_size == 0 || reserved == 0 {
        return None;
    }
    if root_cluster < 2 || root_cluster >= EOC_MIN {
        return None;
    }
    Some(Bpb {
        bytes_per_sector,
        sectors_per_cluster,
        reserved_sectors: reserved,
        num_fats,
        fat_size_sectors: fat_size,
        root_cluster,
        total_sectors: if total != 0 { total } else { u32_at(boot, 32) },
    })
}

/// 簇号 → 扇区号（簇号从 2 开始）。
pub fn cluster_to_sector(bpb: &Bpb, cluster: u32) -> Option<u64> {
    if cluster < 2 || cluster >= EOC_MIN {
        return None;
    }
    Some(bpb.first_data_sector() + (cluster as u64 - 2) * bpb.sectors_per_cluster as u64)
}

/// 读 FAT 表项（低 28 位有效）。
pub fn fat_entry(fat: &[u8], cluster: u32) -> Option<u32> {
    let off = cluster as usize * 4;
    if off + 4 > fat.len() {
        return None;
    }
    Some(u32_at(fat, off) & 0x0FFF_FFFF)
}

pub fn is_eoc(v: u32) -> bool {
    v >= EOC_MIN
}
pub fn is_bad(v: u32) -> bool {
    v == BAD_CLUSTER
}
pub fn is_free(v: u32) -> bool {
    v == FREE_CLUSTER
}

/// 沿簇链走 `max` 步，返回簇号序列（只读遍历，不修改 FAT）。
pub fn walk_chain(fat: &[u8], start: u32, out: &mut [u32]) -> usize {
    let mut cur = start;
    let mut n = 0usize;
    while n < out.len() {
        if cur < 2 {
            break;
        }
        match fat_entry(fat, cur) {
            Some(v) => {
                if is_eoc(v) {
                    out[n] = cur;
                    n += 1;
                    break;
                }
                if is_bad(v) || v < 2 {
                    break;
                }
                out[n] = cur;
                n += 1;
                cur = v;
            }
            None => break,
        }
    }
    n
}

// ---------------------------------------------------------------------------
// F052 FAT32 读写驱动 — 簇分配与 FAT 回写
// ---------------------------------------------------------------------------

/// 找一个空闲簇（从 `hint` 开始线性扫描）。
pub fn alloc_cluster(fat: &[u8], hint: u32) -> Option<u32> {
    let count = (fat.len() / 4) as u32;
    if count < 3 {
        return None;
    }
    for i in 0..count {
        let c = ((hint.saturating_sub(2)) + i) % (count - 2) + 2;
        if let Some(v) = fat_entry(fat, c) {
            if is_free(v) {
                return Some(c);
            }
        }
    }
    None
}

pub fn set_fat_entry(fat: &mut [u8], cluster: u32, value: u32) -> bool {
    let off = cluster as usize * 4;
    if off + 4 > fat.len() {
        return false;
    }
    // 高 4 位保留，写入时不得破坏。
    let keep = u32_at(fat, off) & 0xF000_0000;
    let v = keep | (value & 0x0FFF_FFFF);
    fat[off] = v as u8;
    fat[off + 1] = (v >> 8) as u8;
    fat[off + 2] = (v >> 16) as u8;
    fat[off + 3] = (v >> 24) as u8;
    true
}

/// 把 `chain` 串成一条链并写回 FAT（末尾写 EOC）。
pub fn link_chain(fat: &mut [u8], chain: &[u32]) -> bool {
    if chain.is_empty() {
        return false;
    }
    for i in 0..chain.len() {
        let next = if i + 1 < chain.len() { chain[i + 1] } else { 0x0FFF_FFFF };
        if !set_fat_entry(fat, chain[i], next) {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// F057 文件/目录枚举 — 短目录项
// ---------------------------------------------------------------------------

pub const ATTR_READ_ONLY: u8 = 0x01;
pub const ATTR_HIDDEN: u8 = 0x02;
pub const ATTR_SYSTEM: u8 = 0x04;
pub const ATTR_VOLUME_ID: u8 = 0x08;
pub const ATTR_DIRECTORY: u8 = 0x10;
pub const ATTR_ARCHIVE: u8 = 0x20;
pub const ATTR_LONG_NAME: u8 = 0x0F;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirEntry {
    pub name: [u8; 11],
    pub attr: u8,
    pub cluster: u32,
    pub size: u32,
    pub wrt_date: u16,
    pub wrt_time: u16,
}

impl DirEntry {
    pub fn parse(raw: &[u8]) -> Option<DirEntry> {
        if raw.len() < DIR_ENTRY_SIZE {
            return None;
        }
        let mut name = [0u8; 11];
        name.copy_from_slice(&raw[0..11]);
        let cluster = ((u16_at(raw, 20) as u32) << 16) | u16_at(raw, 26) as u32;
        Some(DirEntry {
            name,
            attr: raw[11],
            cluster,
            size: u32_at(raw, 28),
            wrt_date: u16_at(raw, 24),
            wrt_time: u16_at(raw, 22),
        })
    }

    pub fn encode(&self, out: &mut [u8]) -> bool {
        if out.len() < DIR_ENTRY_SIZE {
            return false;
        }
        for b in out.iter_mut().take(DIR_ENTRY_SIZE) {
            *b = 0;
        }
        out[0..11].copy_from_slice(&self.name);
        out[11] = self.attr;
        out[20..22].copy_from_slice(&((self.cluster >> 16) as u16).to_le_bytes());
        out[22..24].copy_from_slice(&self.wrt_time.to_le_bytes());
        out[24..26].copy_from_slice(&self.wrt_date.to_le_bytes());
        out[26..28].copy_from_slice(&((self.cluster & 0xFFFF) as u16).to_le_bytes());
        out[28..32].copy_from_slice(&self.size.to_le_bytes());
        true
    }

    /// 0x00 = 目录结束；0xE5 = 已删除。
    pub fn is_free(&self) -> bool {
        self.name[0] == 0x00
    }
    pub fn is_deleted(&self) -> bool {
        self.name[0] == 0xE5
    }
    pub fn is_lfn(&self) -> bool {
        self.attr == ATTR_LONG_NAME
    }
    pub fn is_dir(&self) -> bool {
        self.attr & ATTR_DIRECTORY != 0
    }
    pub fn is_volume_id(&self) -> bool {
        self.attr & ATTR_VOLUME_ID != 0
    }

    /// 8.3 名字还原为可读形式（`NAME    EXT` → `NAME.EXT`）。
    pub fn short_name(&self, out: &mut [u8]) -> usize {
        let mut n = 0usize;
        for i in 0..8 {
            if self.name[i] == b' ' {
                break;
            }
            if n < out.len() {
                out[n] = self.name[i];
                n += 1;
            }
        }
        if self.name[8] != b' ' && n < out.len() {
            out[n] = b'.';
            n += 1;
            for i in 8..11 {
                if self.name[i] == b' ' {
                    break;
                }
                if n < out.len() {
                    out[n] = self.name[i];
                    n += 1;
                }
            }
        }
        n
    }

    /// 短名校验和（LFN 用它绑定自己的短目录项）。
    pub fn checksum(&self) -> u8 {
        short_name_checksum(&self.name)
    }
}

pub fn short_name_checksum(name11: &[u8; 11]) -> u8 {
    let mut sum = 0u8;
    for &b in name11.iter() {
        sum = ((sum & 1) << 7).wrapping_add(sum >> 1).wrapping_add(b);
    }
    sum
}

// ---------------------------------------------------------------------------
// F058 文件创建/删除/重命名
// ---------------------------------------------------------------------------

/// 生成一个 8.3 目录项（不做大小写折叠策略，交给上层）。
pub fn make_dir_entry(name83: &[u8; 11], attr: u8, cluster: u32, size: u32) -> DirEntry {
    DirEntry { name: *name83, attr, cluster, size, wrt_date: 0, wrt_time: 0 }
}

/// 删除 = 首字节置 0xE5（Windows 语义，可恢复工具仍能看到）。
pub fn mark_deleted(entry: &mut DirEntry) {
    entry.name[0] = 0xE5;
}

/// 重命名时保留属性与簇号，只换名字。
pub fn rename_entry(entry: &mut DirEntry, new_name: &[u8; 11]) -> bool {
    if new_name[0] == 0x00 || new_name[0] == 0xE5 {
        return false;
    }
    entry.name = *new_name;
    true
}

// ---------------------------------------------------------------------------
// F060 长文件名（LFN/VFAT）
// ---------------------------------------------------------------------------

pub const MAX_LFN_BYTES: usize = 255;

pub const MAX_LFN_ENTRIES: usize = 20;

/// LFN 目录项在磁盘上按**倒序**存放（带 0x40 的末项在最前），
/// 因此本装配器按序号把 13 个码元写回各自的槽位，而不是按喂入顺序拼接。
pub struct LfnAssembler {
    units: [u16; MAX_LFN_ENTRIES * 13],
    total: u8,
    seen: u32,
    checksum: u8,
    buf: [u8; MAX_LFN_BYTES],
    len: usize,
}

impl LfnAssembler {
    pub const fn new() -> LfnAssembler {
        LfnAssembler {
            units: [0u16; MAX_LFN_ENTRIES * 13],
            total: 0,
            seen: 0,
            checksum: 0,
            buf: [0u8; MAX_LFN_BYTES],
            len: 0,
        }
    }

    pub fn reset(&mut self) {
        self.units = [0u16; MAX_LFN_ENTRIES * 13];
        self.total = 0;
        self.seen = 0;
        self.checksum = 0;
        self.buf = [0u8; MAX_LFN_BYTES];
        self.len = 0;
    }

    /// 喂一个 LFN 目录项（32 字节）。返回 true 表示该项被接受。
    pub fn feed(&mut self, raw: &[u8]) -> bool {
        if raw.len() < DIR_ENTRY_SIZE || raw[11] != ATTR_LONG_NAME {
            return false;
        }
        let seq = raw[0];
        let idx = seq & 0x3F;
        if idx == 0 || idx as usize > MAX_LFN_ENTRIES {
            return false;
        }
        if seq & 0x40 != 0 {
            self.reset();
            self.total = idx;
            self.checksum = raw[13];
        } else if self.total == 0 || idx > self.total {
            return false;
        }
        // name1(5) + name2(6) + name3(2) = 13 UTF-16 码元。
        let mut units = [0u16; 13];
        let mut n = 0usize;
        for i in 0..5 {
            units[n] = u16_at(raw, 1 + i * 2);
            n += 1;
        }
        for i in 0..6 {
            units[n] = u16_at(raw, 14 + i * 2);
            n += 1;
        }
        for i in 0..2 {
            units[n] = u16_at(raw, 28 + i * 2);
            n += 1;
        }
        let base = (idx as usize - 1) * 13;
        self.units[base..base + 13].copy_from_slice(&units);
        self.seen |= 1u32 << (idx as u32 - 1);
        true
    }

    /// 所有槽位都到齐才算拼完。
    pub fn complete(&self) -> bool {
        self.total > 0 && self.seen == (1u32 << self.total) - 1
    }

    /// 组装为 UTF-8；未完成返回 Err。
    pub fn finish(&mut self) -> Result<&str, ()> {
        if !self.complete() {
            return Err(());
        }
        self.len = 0;
        // 槽位按序号升序拼接（序号 1 在前）；每个槽位内部遇到 0x0000 / 0xFFFF 收尾。
        for s in 0..(self.total as usize) {
            let base = s * 13;
            for i in 0..13 {
                let u = self.units[base + i];
                if u == 0 || u == 0xFFFF {
                    break;
                }
                self.push_utf8(u)?;
            }
        }
        core::str::from_utf8(&self.buf[..self.len]).map_err(|_| ())
    }

    fn push_utf8(&mut self, u: u16) -> Result<(), ()> {
        {
            if u == 0 || u == 0xFFFF {
                return Ok(());
            }
            for &b in utf16_unit_to_utf8(u).iter() {
                if b == 0 {
                    continue;
                }
                if self.len >= MAX_LFN_BYTES {
                    return Err(());
                }
                self.buf[self.len] = b;
                self.len += 1;
            }
        }
        Ok(())
    }

    pub fn checksum(&self) -> u8 {
        self.checksum
    }
}

impl Default for LfnAssembler {
    fn default() -> Self {
        LfnAssembler::new()
    }
}

/// 单个 UTF-16 码元 → UTF-8（BMP 内的简单映射，代理对不做合并——FAT 文件名也不该用）。
fn utf16_unit_to_utf8(u: u16) -> [u8; 4] {
    if u < 0x80 {
        [u as u8, 0, 0, 0]
    } else if u < 0x800 {
        [0xC0 | (u >> 6) as u8, 0x80 | (u & 0x3F) as u8, 0, 0]
    } else {
        [
            0xE0 | (u >> 12) as u8,
            0x80 | ((u >> 6) & 0x3F) as u8,
            0x80 | (u & 0x3F) as u8,
            0,
        ]
    }
}

// ---------------------------------------------------------------------------
// F061 时间戳与权限映射 — 与 Windows 对齐
// ---------------------------------------------------------------------------

pub const DOS_EPOCH_YEAR: u32 = 1980;
/// 1970-01-01 → 1980-01-01 之间的天数（10 年含 1972/1976 两个闰年）。
pub const DOS_EPOCH_OFFSET_DAYS: u64 = 3652;
const SECS_PER_DAY: u64 = 86_400;

fn is_leap(y: u32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

const MONTH_DAYS: [u16; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

/// DOS 日期/时间 → Unix 秒。非法日期返回 0（不猜）。
pub fn dos_to_unix(date: u16, time: u16) -> u64 {
    let year = ((date >> 9) & 0x7F) as u32 + DOS_EPOCH_YEAR;
    let month = ((date >> 5) & 0x0F) as u32;
    let day = (date & 0x1F) as u32;
    if month == 0 || month > 12 || day == 0 {
        return 0;
    }
    let dim = if month == 2 && is_leap(year) { 29 } else { MONTH_DAYS[(month - 1) as usize] as u32 };
    if day > dim {
        return 0;
    }
    let mut days = DOS_EPOCH_OFFSET_DAYS;
    let mut y = DOS_EPOCH_YEAR;
    while y < year {
        days += if is_leap(y) { 366 } else { 365 };
        y += 1;
    }
    let mut m = 1u32;
    while m < month {
        days += if m == 2 && is_leap(year) { 29 } else { MONTH_DAYS[(m - 1) as usize] as u64 };
        m += 1;
    }
    days += (day - 1) as u64;

    let hour = ((time >> 11) & 0x1F) as u64;
    let min = ((time >> 5) & 0x3F) as u64;
    let sec = ((time & 0x1F) as u64) * 2;
    if hour > 23 || min > 59 || sec > 60 {
        return 0;
    }
    days * SECS_PER_DAY + hour * 3600 + min * 60 + sec
}

/// Unix 秒 → DOS 日期/时间。超出 DOS 2107 上限返回 None（如实失败）。
pub fn unix_to_dos(ts: u64) -> Option<(u16, u16)> {
    let days = ts / SECS_PER_DAY;
    let rem = ts % SECS_PER_DAY;
    let mut y = 1970u32;
    let mut left = days;
    loop {
        let len = if is_leap(y) { 366 } else { 365 };
        if left < len {
            break;
        }
        left -= len;
        y += 1;
        if y > 2107 {
            return None;
        }
    }
    let year = y;
    if year < DOS_EPOCH_YEAR {
        return None;
    }
    let mut month = 1u32;
    loop {
        let dim = if month == 2 && is_leap(year) { 29 } else { MONTH_DAYS[(month - 1) as usize] as u64 };
        if left < dim {
            break;
        }
        left -= dim;
        month += 1;
        if month > 12 {
            return None;
        }
    }
    let day = left as u32 + 1;
    let hour = (rem / 3600) as u32;
    let min = ((rem % 3600) / 60) as u32;
    let sec = (rem % 60) as u32;
    let date = (((year - DOS_EPOCH_YEAR) << 9) & 0xFE00) | ((month << 5) & 0x01E0) | (day & 0x1F);
    let time = ((hour << 11) & 0xF800) | ((min << 5) & 0x07E0) | ((sec / 2) & 0x1F);
    Some((date as u16, time as u16))
}

/// 权限映射：FAT 只有只读位，Windows 的其余 ACL 无法表达——如实返回「不可表达」。
pub fn attr_to_mode(attr: u8) -> u16 {
    if attr & ATTR_READ_ONLY != 0 {
        0o444
    } else {
        0o666
    }
}

// ---------------------------------------------------------------------------
// F063 损坏检测与修复提示
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsHealth {
    Clean,
    /// 可只读挂载的软损坏（交叉链、坏簇）。
    Corrupt,
    /// BPB 级别错误，无法挂载。
    Unmountable,
}

/// FAT 健康检查：保留项必须为 EOC/保留值，且不能出现自环或指向自身的簇。
pub fn fat_health(bpb: &Bpb, fat: &[u8]) -> FsHealth {
    let expect = (bpb.fat_size_sectors as usize * bpb.bytes_per_sector as usize) / 4;
    if fat.len() < expect || expect < 3 {
        return FsHealth::Unmountable;
    }
    match (fat_entry(fat, 0), fat_entry(fat, 1)) {
        (Some(c0), Some(c1)) if is_eoc(c0) && is_eoc(c1) => {}
        _ => return FsHealth::Corrupt,
    }
    let limit = core::cmp::min(expect, bpb.max_cluster() as usize);
    for c in 2..limit {
        if let Some(v) = fat_entry(fat, c as u32) {
            if v == c as u32 {
                return FsHealth::Corrupt; // 自环
            }
            if !is_eoc(v) && !is_bad(v) && !is_free(v) && (v < 2 || v as usize >= limit) {
                return FsHealth::Corrupt; // 越界指针
            }
        }
    }
    FsHealth::Clean
}

// ---------------------------------------------------------------------------
// F067 路径安全 — 越界/`..` 拒绝
// ---------------------------------------------------------------------------

pub const MAX_PATH: usize = 260;

/// 共享卷路径必须绝对、无 `..`、无空段、无驱动号。
pub fn path_safe(path: &str) -> bool {
    let b = path.as_bytes();
    if b.is_empty() || b[0] != b'/' || b.len() > MAX_PATH {
        return false;
    }
    for seg in b.split(|&c| c == b'/') {
        if seg.is_empty() {
            continue;
        }
        if seg == b"." || seg == b".." {
            return false;
        }
        if seg.contains(&b':') || seg.contains(&b'\\') {
            return false;
        }
    }
    true
}

/// 把相对路径解析到根目录之下，越界即拒绝（返回 false）。
pub fn resolve_child(root_depth: usize, path: &str) -> Option<usize> {
    if !path_safe(path) {
        return None;
    }
    let mut depth = root_depth;
    for seg in path.as_bytes().split(|&c| c == b'/') {
        if seg.is_empty() || seg == b"." {
            continue;
        }
        depth += 1;
    }
    Some(depth)
}

// ---------------------------------------------------------------------------
// F071 UTF-8 / UTF-16 文件名编码对齐
// ---------------------------------------------------------------------------

/// UTF-8 → UTF-16LE 字节流（BMP 内）。返回写入字节数，溢出返回 None。
pub fn utf8_to_utf16(s: &str, out: &mut [u8]) -> Option<usize> {
    let mut n = 0usize;
    for ch in s.chars() {
        let u = ch as u32;
        let unit = if u <= 0xFFFF { u as u16 } else { return None };
        if n + 2 > out.len() {
            return None;
        }
        out[n] = unit as u8;
        out[n + 1] = (unit >> 8) as u8;
        n += 2;
    }
    Some(n)
}

/// UTF-16LE 字节流 → UTF-8。返回写入字节数。
pub fn utf16_to_utf8(units: &[u16], out: &mut [u8]) -> usize {
    let mut n = 0usize;
    for &u in units.iter() {
        if u == 0 {
            break;
        }
        for &b in utf16_unit_to_utf8(u).iter() {
            if b == 0 {
                continue;
            }
            if n >= out.len() {
                return n;
            }
            out[n] = b;
            n += 1;
        }
    }
    n
}

// ---------------------------------------------------------------------------
// F072 隐藏文件/回收站语义对齐
// ---------------------------------------------------------------------------

pub const RECYCLE_DIR: &str = "/$RECYCLE.BIN";

pub fn is_hidden(attr: u8) -> bool {
    attr & ATTR_HIDDEN != 0
}

pub fn is_system(attr: u8) -> bool {
    attr & ATTR_SYSTEM != 0
}

/// 常规枚举是否应当跳过该目录项（卷标、LFN 槽、已删除）。
pub fn skip_in_listing(entry: &DirEntry) -> bool {
    entry.is_free() || entry.is_deleted() || entry.is_lfn() || entry.is_volume_id()
}

/// 回收站内的 `$I` 元数据文件与 `$R` 数据文件配对名。
pub fn recycle_names(index: u32, out_i: &mut [u8], out_r: &mut [u8]) -> bool {
    let mut n;
    let prefix_i = b"$I";
    let prefix_r = b"$R";
    if out_i.len() < 16 || out_r.len() < 16 {
        return false;
    }
    out_i[..prefix_i.len()].copy_from_slice(prefix_i);
    out_r[..prefix_r.len()].copy_from_slice(prefix_r);
    n = prefix_i.len();
    let hex = b"0123456789ABCDEF";
    for shift in (0..8).rev() {
        out_i[n] = hex[((index >> (shift * 4)) & 0xF) as usize];
        out_r[n] = out_i[n];
        n += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// F073 文件句柄生命周期
// ---------------------------------------------------------------------------

pub const MAX_HANDLES: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileHandle {
    pub id: u32,
    /// 生成号：句柄复用时递增，旧的悬垂句柄立刻失效。
    pub generation: u32,
    pub first_cluster: u32,
    pub size: u32,
    pub writable: bool,
}

pub struct HandleTable {
    handles: [Option<FileHandle>; MAX_HANDLES],
    next_generation: u32,
}

impl HandleTable {
    pub const fn new() -> HandleTable {
        HandleTable { handles: [None; MAX_HANDLES], next_generation: 1 }
    }

    pub fn open(&mut self, first_cluster: u32, size: u32, writable: bool) -> Option<FileHandle> {
        let slot = (0..MAX_HANDLES).find(|i| self.handles[*i].is_none())?;
        let h = FileHandle {
            id: slot as u32,
            generation: self.next_generation,
            first_cluster,
            size,
            writable,
        };
        self.next_generation = self.next_generation.wrapping_add(1);
        self.handles[slot] = Some(h);
        Some(h)
    }

    /// 校验句柄仍然有效（生成号必须完全匹配）。
    pub fn valid(&self, h: &FileHandle) -> bool {
        match self.handles.get(h.id as usize) {
            Some(Some(cur)) => cur.generation == h.generation,
            _ => false,
        }
    }

    pub fn close(&mut self, h: &FileHandle) -> bool {
        if !self.valid(h) {
            return false;
        }
        self.handles[h.id as usize] = None;
        true
    }

    pub fn open_count(&self) -> usize {
        (0..MAX_HANDLES).filter(|i| self.handles[*i].is_some()).count()
    }
}

impl Default for HandleTable {
    fn default() -> Self {
        HandleTable::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_bpb() -> [u8; 512] {
        let mut b = [0u8; 512];
        b[11..13].copy_from_slice(&512u16.to_le_bytes());
        b[13] = 8;
        b[14..16].copy_from_slice(&32u16.to_le_bytes());
        b[16] = 2;
        b[32..36].copy_from_slice(&1_000_000u32.to_le_bytes());
        b[36..40].copy_from_slice(&1024u32.to_le_bytes());
        b[44..48].copy_from_slice(&2u32.to_le_bytes());
        b[510] = 0x55;
        b[511] = 0xAA;
        b
    }

    #[test]
    fn f051_bpb_parses() {
        let b = sample_bpb();
        let bpb = parse_bpb(&b).unwrap();
        assert_eq!(bpb.bytes_per_sector, 512);
        assert_eq!(bpb.cluster_bytes(), 4096);
        assert_eq!(bpb.first_data_sector(), 32 + 2 * 1024);
        assert_eq!(cluster_to_sector(&bpb, 2), Some(32 + 2048));
        assert!(cluster_to_sector(&bpb, 1).is_none());
    }

    #[test]
    fn f051_rejects_bad_signature() {
        let mut b = sample_bpb();
        b[510] = 0;
        assert!(parse_bpb(&b).is_none());
    }

    #[test]
    fn f051_walks_chain() {
        let mut fat = [0u8; 64];
        set_fat_entry(&mut fat, 0, 0x0FFF_FFF8);
        set_fat_entry(&mut fat, 1, 0xFFFF_FFFF);
        set_fat_entry(&mut fat, 2, 3);
        set_fat_entry(&mut fat, 3, 4);
        set_fat_entry(&mut fat, 4, 0x0FFF_FFFF);
        let mut out = [0u32; 8];
        assert_eq!(walk_chain(&fat, 2, &mut out), 3);
        assert_eq!(out[0], 2);
        assert_eq!(out[2], 4);
    }

    #[test]
    fn f052_alloc_and_link() {
        let mut fat = [0u8; 64];
        set_fat_entry(&mut fat, 0, 0x0FFF_FFF8);
        set_fat_entry(&mut fat, 1, 0xFFFF_FFFF);
        let a = alloc_cluster(&fat, 2).unwrap();
        set_fat_entry(&mut fat, a, 0x0FFF_FFFF);
        let b = alloc_cluster(&fat, a).unwrap();
        assert_ne!(a, b);
        assert!(link_chain(&mut fat, &[a, b]));
        assert_eq!(fat_entry(&fat, a), Some(b));
        assert!(is_eoc(fat_entry(&fat, b).unwrap()));
    }

    #[test]
    fn f057_dir_entry_roundtrip() {
        let e = make_dir_entry(b"HELLO   TXT", ATTR_ARCHIVE, 7, 1234);
        let mut raw = [0u8; DIR_ENTRY_SIZE];
        assert!(e.encode(&mut raw));
        let back = DirEntry::parse(&raw).unwrap();
        assert_eq!(back.cluster, 7);
        assert_eq!(back.size, 1234);
        let mut name = [0u8; 16];
        let n = back.short_name(&mut name);
        assert_eq!(core::str::from_utf8(&name[..n]).unwrap(), "HELLO.TXT");
    }

    #[test]
    fn f058_delete_and_rename() {
        let mut e = make_dir_entry(b"A       TXT", ATTR_ARCHIVE, 5, 1);
        mark_deleted(&mut e);
        assert!(e.is_deleted());
        assert!(rename_entry(&mut e, b"B       TXT"));
        assert!(!e.is_deleted());
        let mut name = [0u8; 16];
        let n = e.short_name(&mut name);
        assert_eq!(core::str::from_utf8(&name[..n]).unwrap(), "B.TXT");
    }

    #[test]
    fn f060_lfn_assembles() {
        let mut asm = LfnAssembler::new();
        // 磁盘顺序：先 0x42（末项，装 "名"），再 0x01（首项，装 "中文"）
        let mut last = [0u8; DIR_ENTRY_SIZE];
        last[0] = 0x42;
        last[11] = ATTR_LONG_NAME;
        last[13] = 0x77;
        let last_units: [u16; 13] = {
            let mut u = [0u16; 13];
            u[0] = 0x540D; // 名
            u
        };
        write_lfn_units(&mut last, &last_units);

        let mut first = [0u8; DIR_ENTRY_SIZE];
        first[0] = 0x01;
        first[11] = ATTR_LONG_NAME;
        first[13] = 0x77;
        let first_units: [u16; 13] = {
            let mut u = [0u16; 13];
            u[0] = 0x4E2D; // 中
            u[1] = 0x6587; // 文
            u
        };
        write_lfn_units(&mut first, &first_units);

        assert!(asm.feed(&last));
        assert!(asm.feed(&first));
        assert!(asm.complete());
        assert_eq!(asm.finish(), Ok("中文名"));
        assert_eq!(asm.checksum(), 0x77);
    }

    fn write_lfn_units(raw: &mut [u8], units: &[u16; 13]) {
        for i in 0..5 {
            raw[1 + i * 2..3 + i * 2].copy_from_slice(&units[i].to_le_bytes());
        }
        for i in 0..6 {
            raw[14 + i * 2..16 + i * 2].copy_from_slice(&units[5 + i].to_le_bytes());
        }
        for i in 0..2 {
            raw[28 + i * 2..30 + i * 2].copy_from_slice(&units[11 + i].to_le_bytes());
        }
    }

    #[test]
    fn f061_timestamp_roundtrip() {
        // DOS 时间戳起点是 1980，1970 年无法表达——如实返回 None，不伪造。
        assert!(unix_to_dos(0).is_none());
        let (d, t) = unix_to_dos(1_600_000_000).unwrap();
        assert_eq!(dos_to_unix(d, t), 1_600_000_000);
        // 奇数秒会被截断到偶数秒（DOS 只有 2 秒分辨率）—如实降级。
        let (d2, t2) = unix_to_dos(1_600_000_001).unwrap();
        assert_eq!(dos_to_unix(d2, t2), 1_600_000_000);
        assert!(dos_to_unix(0, 0) == 0);
        assert!(unix_to_dos(u64::MAX / 1000).is_none(), "beyond 2107 is refused");
    }

    #[test]
    fn f063_detects_self_loop() {
        let bpb = small_bpb();
        let mut fat = [0u8; SMALL_FAT_BYTES];
        set_fat_entry(&mut fat, 0, 0x0FFF_FFF8);
        set_fat_entry(&mut fat, 1, 0xFFFF_FFFF);
        assert_eq!(fat_health(&bpb, &fat), FsHealth::Clean);
        set_fat_entry(&mut fat, 3, 3);
        assert_eq!(fat_health(&bpb, &fat), FsHealth::Corrupt);
        // FAT 太短 → 无法挂载（不猜表长）
        assert_eq!(fat_health(&bpb, &[0u8; 8]), FsHealth::Unmountable);
    }

    /// 2 个 FAT 扇区 = 256 个簇，单测里放得下。
    pub const SMALL_FAT_BYTES: usize = 2 * 512;

    fn small_bpb() -> Bpb {
        let mut b = sample_bpb();
        b[36..40].copy_from_slice(&2u32.to_le_bytes());
        parse_bpb(&b).unwrap()
    }

    #[test]
    fn f067_path_rules() {
        assert!(path_safe("/docs/readme.md"));
        assert!(!path_safe("/docs/../secret"));
        assert!(!path_safe("docs"));
        assert!(!path_safe("/C:/x"));
        assert_eq!(resolve_child(0, "/a/b"), Some(2));
        assert_eq!(resolve_child(0, "/a/../b"), None);
    }

    #[test]
    fn f071_utf_conversion() {
        let mut u16buf = [0u8; 32];
        let n = utf8_to_utf16("中文ab", &mut u16buf).unwrap();
        assert_eq!(n, 8);
        let mut units = [0u16; 8];
        for i in 0..4 {
            units[i] = u16::from_le_bytes([u16buf[i * 2], u16buf[i * 2 + 1]]);
        }
        let mut back = [0u8; 32];
        let m = utf16_to_utf8(&units, &mut back);
        assert_eq!(core::str::from_utf8(&back[..m]).unwrap(), "中文ab");
    }

    #[test]
    fn f072_recycle_pairing() {
        assert!(is_hidden(ATTR_HIDDEN));
        assert!(is_system(ATTR_SYSTEM));
        let mut i = [0u8; 16];
        let mut r = [0u8; 16];
        assert!(recycle_names(0x0000_00FF, &mut i, &mut r));
        assert_eq!(&i[..10], b"$I000000FF");
        assert_eq!(&r[..10], b"$R000000FF");
    }

    #[test]
    fn f073_stale_handle_rejected() {
        let mut t = HandleTable::new();
        let h = t.open(5, 10, true).unwrap();
        assert!(t.valid(&h));
        assert!(t.close(&h));
        assert!(!t.valid(&h));
        let h2 = t.open(9, 1, false).unwrap();
        assert_ne!(h.generation, h2.generation);
        assert_eq!(t.open_count(), 1);
    }
}
