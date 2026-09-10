//! TRINITY-500 · AI-03 共享卷文件系统域（部分）：exFAT 读写驱动（F053/F054/F059/F074）
//!
//! 选 exFAT 的唯一理由是 **>4GB 大文件**（F059）：共享卷要放媒体与镜像，
//! FAT32 的 32 位 size 字段不够用，exFAT 的 Stream Extension 用 u64 表达。

// ---------------------------------------------------------------------------
// F053 exFAT 只读驱动 — 主引导参数解析
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExfatBpb {
    pub bytes_per_sector_shift: u8,
    pub sectors_per_cluster_shift: u8,
    pub fat_offset_sector: u32,
    pub fat_length_sectors: u32,
    pub cluster_heap_offset_sector: u32,
    pub cluster_count: u32,
    pub root_dir_cluster: u32,
    pub volume_serial: u32,
    pub volume_flags: u16,
    pub number_of_fats: u8,
}

impl ExfatBpb {
    pub fn bytes_per_sector(&self) -> u32 {
        1u32 << self.bytes_per_sector_shift
    }
    pub fn sectors_per_cluster(&self) -> u32 {
        1u32 << self.sectors_per_cluster_shift
    }
    pub fn cluster_bytes(&self) -> u64 {
        self.bytes_per_sector() as u64 * self.sectors_per_cluster() as u64
    }
}

fn u32_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

pub const EXFAT_SIGNATURE: &[u8; 8] = b"EXFAT   ";

/// 解析 exFAT 主引导扇区。文件系统名不对、位移越界一律拒绝。
pub fn parse_exfat_bpb(boot: &[u8]) -> Option<ExfatBpb> {
    if boot.len() < 512 {
        return None;
    }
    if boot[510] != 0x55 || boot[511] != 0xAA {
        return None;
    }
    if &boot[3..11] != EXFAT_SIGNATURE {
        return None;
    }
    let bps_shift = boot[0x6C];
    let spc_shift = boot[0x6D];
    if !(9..=12).contains(&bps_shift) {
        return None;
    }
    if spc_shift > 25 - bps_shift {
        return None;
    }
    let bpb = ExfatBpb {
        bytes_per_sector_shift: bps_shift,
        sectors_per_cluster_shift: spc_shift,
        fat_offset_sector: u32_at(boot, 0x50),
        fat_length_sectors: u32_at(boot, 0x54),
        cluster_heap_offset_sector: u32_at(boot, 0x58),
        cluster_count: u32_at(boot, 0x5C),
        root_dir_cluster: u32_at(boot, 0x60),
        volume_serial: u32_at(boot, 0x64),
        volume_flags: u16::from_le_bytes([boot[0x6A], boot[0x6B]]),
        number_of_fats: boot[0x6E],
    };
    if bpb.cluster_count < 2 || bpb.root_dir_cluster < 2 || bpb.root_dir_cluster >= bpb.cluster_count + 2 {
        return None;
    }
    Some(bpb)
}

/// 簇号 → 扇区（簇 2 是堆里的第一个簇）。
pub fn cluster_to_sector(bpb: &ExfatBpb, cluster: u32) -> Option<u64> {
    if cluster < 2 || cluster > bpb.cluster_count + 1 {
        return None;
    }
    let rel = (cluster as u64 - 2) << bpb.sectors_per_cluster_shift;
    Some(bpb.cluster_heap_offset_sector as u64 + rel)
}

pub const EXFAT_EOC: u32 = 0xFFFF_FFFF;
pub const EXFAT_BAD: u32 = 0xFFFF_FFF7;

pub fn fat_entry(fat: &[u8], cluster: u32) -> Option<u32> {
    let off = cluster as usize * 4;
    if off + 4 > fat.len() {
        return None;
    }
    Some(u32_at(fat, off))
}

pub fn is_eoc(v: u32) -> bool {
    v >= 0xFFFF_FFF8
}

pub fn is_bad(v: u32) -> bool {
    v == EXFAT_BAD
}

// ---------------------------------------------------------------------------
// F054 exFAT 读写驱动 — 簇分配
// ---------------------------------------------------------------------------

pub fn alloc_cluster(fat: &[u8], hint: u32) -> Option<u32> {
    let count = (fat.len() / 4) as u32;
    if count < 3 {
        return None;
    }
    for i in 0..count {
        let c = (hint.saturating_sub(2) + i) % (count - 2) + 2;
        if let Some(v) = fat_entry(fat, c) {
            if v == 0 {
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
    fat[off..off + 4].copy_from_slice(&value.to_le_bytes());
    true
}

pub fn link_chain(fat: &mut [u8], chain: &[u32]) -> bool {
    if chain.is_empty() {
        return false;
    }
    for i in 0..chain.len() {
        let next = if i + 1 < chain.len() { chain[i + 1] } else { EXFAT_EOC };
        if !set_fat_entry(fat, chain[i], next) {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// F059 大文件（>4GB）支持 — Stream Extension 的 u64 size
// ---------------------------------------------------------------------------

pub const ENTRY_FILE: u8 = 0x85;
pub const ENTRY_STREAM: u8 = 0xC0;
pub const ENTRY_NAME: u8 = 0xC1;
pub const NO_FAT_CHAIN: u8 = 0x02;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamEntry {
    /// bit0 = AllocationPossible，bit1 = NoFatChain。
    pub flags: u8,
    pub first_cluster: u32,
    pub data_length: u64,
    pub valid_data_length: u64,
}

impl StreamEntry {
    pub fn parse(raw: &[u8]) -> Option<StreamEntry> {
        if raw.len() < 32 || raw[0] != ENTRY_STREAM {
            return None;
        }
        Some(StreamEntry {
            flags: raw[1],
            first_cluster: u32_at(raw, 20),
            data_length: u64::from_le_bytes([
                raw[24], raw[25], raw[26], raw[27], raw[28], raw[29], raw[30], raw[31],
            ]),
            valid_data_length: u64::from_le_bytes([
                raw[8], raw[9], raw[10], raw[11], raw[12], raw[13], raw[14], raw[15],
            ]),
        })
    }

    /// NoFatChain：文件在磁盘上物理连续，无需遍历 FAT（大文件的常见形态）。
    pub fn contiguous(&self) -> bool {
        self.flags & NO_FAT_CHAIN != 0
    }

    /// 该文件是否超过 FAT32 的 4GiB 上限。
    pub fn exceeds_fat32(&self) -> bool {
        self.data_length > 0xFFFF_FFFF
    }

    /// 需要的簇数（向上取整）。
    pub fn clusters_needed(&self, cluster_bytes: u64) -> u64 {
        if cluster_bytes == 0 {
            return 0;
        }
        self.data_length / cluster_bytes + u64::from(self.data_length % cluster_bytes != 0)
    }
}

/// 校验流描述自洽：连续文件不能声明 valid > data；非空文件必须有合法首簇。
pub fn stream_sane(s: &StreamEntry) -> bool {
    if s.valid_data_length > s.data_length {
        return false;
    }
    if s.data_length > 0 && s.first_cluster < 2 {
        return false;
    }
    if s.data_length == 0 && s.first_cluster != 0 {
        return false;
    }
    true
}

/// 大文件能否放进这个卷（按簇预算而不是 32 位字节预算）。
pub fn fits_on_volume(bpb: &ExfatBpb, free_clusters: u32, size_bytes: u64) -> bool {
    if size_bytes == 0 {
        return true;
    }
    let need = size_bytes / bpb.cluster_bytes()
        + u64::from(size_bytes % bpb.cluster_bytes() != 0);
    need <= free_clusters as u64
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileEntry {
    pub attr: u16,
}

impl FileEntry {
    pub fn parse(raw: &[u8]) -> Option<FileEntry> {
        if raw.len() < 32 || raw[0] != ENTRY_FILE {
            return None;
        }
        Some(FileEntry { attr: u16::from_le_bytes([raw[4], raw[5]]) })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NameEntry {
    /// 文件名长度（UTF-16 码元数）。
    pub len: u8,
    units: [u16; 15],
}

impl NameEntry {
    pub fn parse(raw: &[u8]) -> Option<NameEntry> {
        if raw.len() < 32 || raw[0] != ENTRY_NAME {
            return None;
        }
        let mut units = [0u16; 15];
        for i in 0..15 {
            units[i] = u16::from_le_bytes([raw[2 + i * 2], raw[3 + i * 2]]);
        }
        Some(NameEntry { len: raw[1], units })
    }

    pub fn units(&self) -> &[u16] {
        &self.units[..core::cmp::min(self.len as usize, 15)]
    }
}

// ---------------------------------------------------------------------------
// F074 文件系统诊断器
// ---------------------------------------------------------------------------

pub const MAX_ISSUES: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IssueKind {
    FatMismatch,
    ClusterOverflow,
    BadClusterInChain,
    OrphanCluster,
    RootOutOfRange,
    ActiveFatMismatch,
}

impl IssueKind {
    pub fn text(self) -> &'static str {
        match self {
            IssueKind::FatMismatch => "fat length does not cover cluster count",
            IssueKind::ClusterOverflow => "chain references a cluster beyond the heap",
            IssueKind::BadClusterInChain => "chain hits a bad cluster",
            IssueKind::OrphanCluster => "cluster is allocated but unreachable from root",
            IssueKind::RootOutOfRange => "root directory cluster outside heap",
            IssueKind::ActiveFatMismatch => "active fat index exceeds number of fats",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct DiagReport {
    issues: [Option<IssueKind>; MAX_ISSUES],
    count: usize,
}

impl DiagReport {
    pub const fn new() -> DiagReport {
        DiagReport { issues: [None; MAX_ISSUES], count: 0 }
    }

    fn push(&mut self, k: IssueKind) {
        if self.count >= MAX_ISSUES {
            return;
        }
        for i in 0..self.count {
            if self.issues[i] == Some(k) {
                return;
            }
        }
        self.issues[self.count] = Some(k);
        self.count += 1;
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, i: usize) -> Option<IssueKind> {
        if i < self.count {
            self.issues[i]
        } else {
            None
        }
    }

    pub fn has(&self, k: IssueKind) -> bool {
        (0..self.count).any(|i| self.issues[i] == Some(k))
    }

    pub fn render(&self, out: &mut [u8]) -> usize {
        let mut n = 0usize;
        for i in 0..self.count {
            if let Some(k) = self.get(i) {
                for &b in k.text().as_bytes() {
                    if n < out.len() {
                        out[n] = b;
                        n += 1;
                    }
                }
                if n < out.len() {
                    out[n] = b'\n';
                    n += 1;
                }
            }
        }
        n
    }
}

impl Default for DiagReport {
    fn default() -> Self {
        DiagReport::new()
    }
}

/// exFAT 卷体检：FAT 覆盖度、根目录范围、活跃 FAT 索引、簇链越界。
pub fn diagnose_volume(bpb: &ExfatBpb, fat: &[u8], active_fat: u8) -> DiagReport {
    let mut r = DiagReport::new();
    let needed = (bpb.cluster_count as u64 + 2) * 4;
    if (bpb.fat_length_sectors as u64) * (bpb.bytes_per_sector() as u64) < needed {
        r.push(IssueKind::FatMismatch);
    }
    if bpb.root_dir_cluster < 2 || bpb.root_dir_cluster > bpb.cluster_count + 1 {
        r.push(IssueKind::RootOutOfRange);
    }
    if bpb.number_of_fats == 0 || active_fat >= bpb.number_of_fats {
        r.push(IssueKind::ActiveFatMismatch);
    }
    let mut c = bpb.root_dir_cluster;
    let mut guard = 0usize;
    while guard < 64 {
        guard += 1;
        match fat_entry(fat, c) {
            Some(v) => {
                if is_bad(v) {
                    r.push(IssueKind::BadClusterInChain);
                    break;
                }
                if is_eoc(v) {
                    break;
                }
                if v < 2 || v > bpb.cluster_count + 1 {
                    r.push(IssueKind::ClusterOverflow);
                    break;
                }
                c = v;
            }
            None => {
                r.push(IssueKind::ClusterOverflow);
                break;
            }
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_boot() -> [u8; 512] {
        let mut b = [0u8; 512];
        b[3..11].copy_from_slice(EXFAT_SIGNATURE);
        b[0x50..0x54].copy_from_slice(&128u32.to_le_bytes());
        b[0x54..0x58].copy_from_slice(&1024u32.to_le_bytes());
        b[0x58..0x5C].copy_from_slice(&4096u32.to_le_bytes());
        b[0x5C..0x60].copy_from_slice(&100_000u32.to_le_bytes());
        b[0x60..0x64].copy_from_slice(&5u32.to_le_bytes());
        b[0x64..0x68].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
        b[0x6A] = 0x00;
        b[0x6C] = 9; // 512
        b[0x6D] = 3; // 8 sectors per cluster
        b[0x6E] = 1;
        b[510] = 0x55;
        b[511] = 0xAA;
        b
    }

    #[test]
    fn f053_bpb_parses() {
        let b = sample_boot();
        let bpb = parse_exfat_bpb(&b).unwrap();
        assert_eq!(bpb.bytes_per_sector(), 512);
        assert_eq!(bpb.sectors_per_cluster(), 8);
        assert_eq!(bpb.cluster_bytes(), 4096);
        assert_eq!(cluster_to_sector(&bpb, 2), Some(4096));
        assert!(cluster_to_sector(&bpb, 1).is_none());
    }

    #[test]
    fn f053_rejects_non_exfat() {
        let mut b = sample_boot();
        b[3..11].copy_from_slice(b"FAT32   ");
        assert!(parse_exfat_bpb(&b).is_none());
    }

    #[test]
    fn f054_alloc_and_link() {
        let mut fat = [0u8; 64];
        set_fat_entry(&mut fat, 0, 0xFFFF_FFF8);
        set_fat_entry(&mut fat, 1, EXFAT_EOC);
        let a = alloc_cluster(&fat, 2).unwrap();
        let b = alloc_cluster(&fat, a + 1).unwrap();
        assert_ne!(a, b);
        assert!(link_chain(&mut fat, &[a, b]));
        assert_eq!(fat_entry(&fat, a), Some(b));
        assert!(is_eoc(fat_entry(&fat, b).unwrap()));
    }

    #[test]
    fn f059_stream_over_4gb() {
        let mut raw = [0u8; 32];
        raw[0] = ENTRY_STREAM;
        raw[1] = NO_FAT_CHAIN;
        raw[20..24].copy_from_slice(&5u32.to_le_bytes());
        let big = 8u64 << 30;
        raw[24..32].copy_from_slice(&big.to_le_bytes());
        raw[8..16].copy_from_slice(&big.to_le_bytes());
        let s = StreamEntry::parse(&raw).unwrap();
        assert!(s.exceeds_fat32());
        assert!(s.contiguous());
        assert!(stream_sane(&s));
        assert_eq!(s.clusters_needed(4096), (8u64 << 30) / 4096);
    }

    #[test]
    fn f059_fits_check() {
        let bpb = parse_exfat_bpb(&sample_boot()).unwrap();
        assert!(fits_on_volume(&bpb, 100_000, 4u64 << 20));
        assert!(!fits_on_volume(&bpb, 10, 4u64 << 20));
    }

    #[test]
    fn f059_stream_sanity() {
        let s = StreamEntry { flags: 0, first_cluster: 0, data_length: 10, valid_data_length: 10 };
        assert!(!stream_sane(&s));
        let s = StreamEntry { flags: 0, first_cluster: 3, data_length: 10, valid_data_length: 20 };
        assert!(!stream_sane(&s));
        let s = StreamEntry { flags: 0, first_cluster: 0, data_length: 0, valid_data_length: 0 };
        assert!(stream_sane(&s));
    }

    #[test]
    fn f074_diagnoses_bad_chain() {
        let bpb = parse_exfat_bpb(&sample_boot()).unwrap();
        let mut fat = [0u8; 64 * 1024];
        set_fat_entry(&mut fat, 5, EXFAT_BAD);
        let r = diagnose_volume(&bpb, &fat, 0);
        assert!(r.has(IssueKind::BadClusterInChain));
        let mut out = [0u8; 256];
        assert!(r.render(&mut out) > 0);
    }

    #[test]
    fn f074_clean_volume_has_no_issues() {
        let bpb = parse_exfat_bpb(&sample_boot()).unwrap();
        let mut fat = [0u8; 64 * 1024];
        set_fat_entry(&mut fat, 5, EXFAT_EOC);
        let r = diagnose_volume(&bpb, &fat, 0);
        assert_eq!(r.len(), 0);
    }

    #[test]
    fn f053_name_entry() {
        let mut raw = [0u8; 32];
        raw[0] = ENTRY_NAME;
        raw[1] = 3;
        let units: [u16; 3] = [0x4E2D, 0x6587, 0x540D];
        for (i, u) in units.iter().enumerate() {
            raw[2 + i * 2..4 + i * 2].copy_from_slice(&u.to_le_bytes());
        }
        let n = NameEntry::parse(&raw).unwrap();
        assert_eq!(n.units(), &units);
        assert!(FileEntry::parse(&raw).is_none());
    }
}
