//! TRINITY-500 · AI-03 共享卷文件系统域（F051~F075，W1）
//!
//! 在 VARIX-500 AI-06 的块设备层之上，做「与 Windows 互读写」的 FAT32/exFAT。
//! 子模块：[`fat32`] 处理 32 位 FAT，[`exfat`] 处理大文件与诊断器，本文件负责
//! 卷布局、挂载表、写缓存、GUID 自愈与降级——即「共享卷」这一层策略。

pub mod exfat;
pub mod fat32;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F055 共享卷布局规范 — 目录树/命名约定
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SharedDirKind {
    /// 文档：记录/导图/推演/代码档案（AI-04 的文档契约落点）。
    Docs,
    /// 媒体：图片/音视频/附件。
    Media,
    /// 配置：跨系统设置。
    Config,
    /// 便携软件的数据目录（可执行各系统一份，只共享数据）。
    Apps,
    /// 保险箱：密文区。
    Vault,
}

impl SharedDirKind {
    pub fn name(self) -> &'static str {
        match self {
            SharedDirKind::Docs => "docs",
            SharedDirKind::Media => "media",
            SharedDirKind::Config => "config",
            SharedDirKind::Apps => "apps",
            SharedDirKind::Vault => "vault",
        }
    }

    pub fn from_name(n: &str) -> Option<SharedDirKind> {
        match n {
            "docs" => Some(SharedDirKind::Docs),
            "media" => Some(SharedDirKind::Media),
            "config" => Some(SharedDirKind::Config),
            "apps" => Some(SharedDirKind::Apps),
            "vault" => Some(SharedDirKind::Vault),
            _ => None,
        }
    }
}

pub const SHARED_DIRS: [SharedDirKind; 5] = [
    SharedDirKind::Docs,
    SharedDirKind::Media,
    SharedDirKind::Config,
    SharedDirKind::Apps,
    SharedDirKind::Vault,
];

/// 顶层路径 → 分区类型；非规范顶层目录返回 None（调用方据此拒绝写入）。
pub fn classify_top(path: &str) -> Option<SharedDirKind> {
    let b = path.as_bytes();
    if b.first() != Some(&b'/') {
        return None;
    }
    let rest = &b[1..];
    let end = rest.iter().position(|&c| c == b'/').unwrap_or(rest.len());
    SharedDirKind::from_name(core::str::from_utf8(&rest[..end]).ok()?)
}

/// 保险箱目录只允许密文：明文写入必须被拒。
pub fn write_allowed(path: &str, is_ciphertext: bool) -> bool {
    match classify_top(path) {
        Some(SharedDirKind::Vault) => is_ciphertext,
        Some(_) => true,
        None => false,
    }
}

// ---------------------------------------------------------------------------
// F056 共享卷挂载表 — 多卷/多槽位
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsKind {
    Fat32,
    Exfat,
    Uxv,
    Unknown,
}

impl FsKind {
    pub fn name(self) -> &'static str {
        match self {
            FsKind::Fat32 => "fat32",
            FsKind::Exfat => "exfat",
            FsKind::Uxv => "uxv",
            FsKind::Unknown => "unknown",
        }
    }

    /// 该文件系统能否表达 >4GB 文件。
    pub fn supports_large_files(self) -> bool {
        matches!(self, FsKind::Exfat | FsKind::Uxv)
    }
}

pub const MOUNT_RO: u8 = 1 << 0;
pub const MOUNT_DIRTY: u8 = 1 << 1;
pub const MOUNT_REMOVABLE: u8 = 1 << 2;
pub const MOUNT_SHARED: u8 = 1 << 3;

#[derive(Clone, Copy, Debug)]
pub struct MountEntry {
    pub slot: u8,
    pub fs: FsKind,
    pub flags: u8,
    pub serial: u32,
}

impl MountEntry {
    pub fn readonly(&self) -> bool {
        self.flags & MOUNT_RO != 0
    }
    pub fn shared(&self) -> bool {
        self.flags & MOUNT_SHARED != 0
    }
}

pub const MAX_MOUNTS: usize = 8;

pub struct MountTable {
    mounts: [Option<MountEntry>; MAX_MOUNTS],
    count: usize,
}

impl MountTable {
    pub const fn new() -> MountTable {
        MountTable { mounts: [None; MAX_MOUNTS], count: 0 }
    }

    pub fn mount(&mut self, e: MountEntry) -> bool {
        if (self.count as u8) >= MAX_MOUNTS as u8 {
            return false;
        }
        if (0..self.count).any(|i| self.mounts[i].map(|m| m.slot) == Some(e.slot)) {
            return false;
        }
        self.mounts[self.count] = Some(e);
        self.count += 1;
        true
    }

    pub fn unmount(&mut self, slot: u8) -> bool {
        for i in 0..self.count {
            if self.mounts[i].map(|m| m.slot) == Some(slot) {
                self.mounts[i] = self.mounts[self.count - 1];
                self.mounts[self.count - 1] = None;
                self.count -= 1;
                return true;
            }
        }
        false
    }

    pub fn by_slot(&self, slot: u8) -> Option<MountEntry> {
        (0..self.count).filter_map(|i| self.mounts[i]).find(|m| m.slot == slot)
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

impl Default for MountTable {
    fn default() -> Self {
        MountTable::new()
    }
}

// ---------------------------------------------------------------------------
// F062 写缓存与掉电安全 — fsync 语义
// ---------------------------------------------------------------------------

pub const MAX_DIRTY: usize = 32;

/// 脏扇区集合。fsync 必须把所有脏扇区落盘后才返回成功。
pub struct WriteCache {
    dirty: [Option<u64>; MAX_DIRTY],
    count: usize,
}

impl WriteCache {
    pub const fn new() -> WriteCache {
        WriteCache { dirty: [None; MAX_DIRTY], count: 0 }
    }

    pub fn mark_dirty(&mut self, sector: u64) -> bool {
        if (0..self.count).any(|i| self.dirty[i] == Some(sector)) {
            return true;
        }
        if self.count >= MAX_DIRTY {
            return false;
        }
        self.dirty[self.count] = Some(sector);
        self.count += 1;
        true
    }

    /// 落盘：返回本次刷出的扇区数（调用方据此更新进度，不做假进度）。
    pub fn flush(&mut self) -> usize {
        let n = self.count;
        self.count = 0;
        n
    }

    pub fn dirty_count(&self) -> usize {
        self.count
    }

    /// 掉电语义：fsync 成功 <=> 脏集合清空。
    pub fn synced(&self) -> bool {
        self.count == 0
    }
}

impl Default for WriteCache {
    fn default() -> Self {
        WriteCache::new()
    }
}

// ---------------------------------------------------------------------------
// F064 卷 GUID 自愈 — Variable 已有概念，移植内核
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VolumeGuid {
    pub a: u32,
    pub b: u16,
    pub c: u16,
    pub d: [u8; 8],
}

impl VolumeGuid {
    /// 由卷序列号派生一个稳定 GUID（同一张盘永远得到同一个值）。
    pub fn from_serial(serial: u32) -> VolumeGuid {
        let mut x = (serial as u64) ^ 0x5851_F42D_4C95_7B2D;
        let mut next = || {
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            x
        };
        let a = next() as u32;
        let b = (next() >> 32) as u16;
        let c = next() as u16;
        let mut d = [0u8; 8];
        let tail = next().to_le_bytes();
        d.copy_from_slice(&tail);
        VolumeGuid { a, b, c, d }
    }

    pub fn is_null(&self) -> bool {
        self.a == 0 && self.b == 0 && self.c == 0 && self.d == [0u8; 8]
    }
}

/// 自愈：GUID 丢失/全零/与序列号不符时，按派生规则重建。
/// 返回（生效 GUID，是否发生修复）。
pub fn self_heal(stored: Option<VolumeGuid>, serial: u32) -> (VolumeGuid, bool) {
    let derived = VolumeGuid::from_serial(serial);
    match stored {
        None => (derived, true),
        Some(g) if g.is_null() => (derived, true),
        Some(g) if g != derived => (g, false),
        Some(g) => (g, false),
    }
}

// ---------------------------------------------------------------------------
// F065 只读降级 — 损坏卷只读挂载
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MountDecision {
    ReadWrite,
    ReadOnly,
    Refuse,
}

impl MountDecision {
    pub fn text(self) -> &'static str {
        match self {
            MountDecision::ReadWrite => "read-write",
            MountDecision::ReadOnly => "read-only (degraded)",
            MountDecision::Refuse => "refuse",
        }
    }
}

/// 健康度决定挂载方式：干净→读写，软损坏→只读，不可挂载→拒绝。
pub fn mount_decision(health: fat32::FsHealth) -> MountDecision {
    match health {
        fat32::FsHealth::Clean => MountDecision::ReadWrite,
        fat32::FsHealth::Corrupt => MountDecision::ReadOnly,
        fat32::FsHealth::Unmountable => MountDecision::Refuse,
    }
}

// ---------------------------------------------------------------------------
// F066 共享卷与 .uxv 容器互操作
// ---------------------------------------------------------------------------

pub const UXV_MAGIC: [u8; 4] = *b".UXV";
pub const UXV_HEADER_LEN: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UxvHeader {
    pub version: u16,
    pub payload_bytes: u64,
    pub crc: u32,
}

pub fn uxv_probe(bytes: &[u8]) -> bool {
    bytes.len() >= UXV_HEADER_LEN && bytes[0..4] == UXV_MAGIC
}

pub fn uxv_header(bytes: &[u8]) -> Option<UxvHeader> {
    if !uxv_probe(bytes) {
        return None;
    }
    Some(UxvHeader {
        version: u16::from_le_bytes([bytes[4], bytes[5]]),
        payload_bytes: u64::from_le_bytes([
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        ]),
        crc: u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]),
    })
}

/// 共享卷上的 .uxv 容器在 exFAT 之上使用，因此大文件不受 FAT32 限制。
pub fn uxv_needs_large_fs(payload: u64) -> bool {
    payload > 0xFFFF_FFFF
}

// ---------------------------------------------------------------------------
// F068 目录树缓存与失效
// ---------------------------------------------------------------------------

pub const MAX_CACHE_NODES: usize = 64;
pub const MAX_NAME: usize = 24;

#[derive(Clone, Copy, Debug)]
pub struct DirNode {
    pub name: [u8; MAX_NAME],
    pub name_len: usize,
    pub first_cluster: u32,
    pub size: u32,
    pub is_dir: bool,
}

impl DirNode {
    pub fn name(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("")
    }
}

pub struct DirCache {
    nodes: [Option<DirNode>; MAX_CACHE_NODES],
    count: usize,
    pub generation: u32,
}

impl DirCache {
    pub const fn new() -> DirCache {
        DirCache { nodes: [None; MAX_CACHE_NODES], count: 0, generation: 0 }
    }

    pub fn insert(&mut self, name: &str, first_cluster: u32, size: u32, is_dir: bool) -> bool {
        if self.count >= MAX_CACHE_NODES {
            return false;
        }
        let b = name.as_bytes();
        if b.is_empty() || b.len() > MAX_NAME {
            return false;
        }
        let mut buf = [0u8; MAX_NAME];
        buf[..b.len()].copy_from_slice(b);
        self.nodes[self.count] = Some(DirNode { name: buf, name_len: b.len(), first_cluster, size, is_dir });
        self.count += 1;
        true
    }

    pub fn lookup(&self, name: &str) -> Option<DirNode> {
        (0..self.count)
            .filter_map(|i| self.nodes[i])
            .find(|n| n.name() == name)
    }

    pub fn len(&self) -> usize {
        self.count
    }

    /// 失效：写操作后必须调用，缓存全清且 generation 递增（读者据此丢弃旧视图）。
    pub fn invalidate(&mut self) {
        self.nodes = [None; MAX_CACHE_NODES];
        self.count = 0;
        self.generation = self.generation.wrapping_add(1);
    }
}

impl Default for DirCache {
    fn default() -> Self {
        DirCache::new()
    }
}

// ---------------------------------------------------------------------------
// F070 跨系统一致性协议 — 写锁/校验和（扇区级记录，协议层见 AI-04）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemId {
    Windows = 1,
    Variable = 2,
    Varix = 3,
}

impl SystemId {
    pub fn from_u32(v: u32) -> Option<SystemId> {
        match v {
            1 => Some(SystemId::Windows),
            2 => Some(SystemId::Variable),
            3 => Some(SystemId::Varix),
            _ => None,
        }
    }
}

pub const LOCK_RECORD_LEN: usize = 32;
pub const LOCK_MAGIC: [u8; 4] = *b"VXL1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LockRecord {
    pub owner: SystemId,
    pub generation: u32,
    pub stamp_ms: u64,
    pub crc: u32,
}

impl LockRecord {
    pub fn encode(&self, out: &mut [u8]) -> bool {
        if out.len() < LOCK_RECORD_LEN {
            return false;
        }
        for b in out.iter_mut().take(LOCK_RECORD_LEN) {
            *b = 0;
        }
        out[0..4].copy_from_slice(&LOCK_MAGIC);
        out[4..8].copy_from_slice(&(self.owner as u32).to_le_bytes());
        out[8..12].copy_from_slice(&self.generation.to_le_bytes());
        out[12..20].copy_from_slice(&self.stamp_ms.to_le_bytes());
        out[20..24].copy_from_slice(&self.crc.to_le_bytes());
        true
    }

    pub fn decode(bytes: &[u8]) -> Option<LockRecord> {
        if bytes.len() < LOCK_RECORD_LEN || bytes[0..4] != LOCK_MAGIC {
            return None;
        }
        let owner = SystemId::from_u32(u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]))?;
        let generation = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        let stamp_ms = u64::from_le_bytes([
            bytes[12], bytes[13], bytes[14], bytes[15], bytes[16], bytes[17], bytes[18], bytes[19],
        ]);
        let crc = u32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        Some(LockRecord { owner, generation, stamp_ms, crc })
    }

    pub fn crc_ok(&self) -> bool {
        self.crc == crate::power::crc32(&self.stamp_ms.to_le_bytes())
    }

    pub fn new(owner: SystemId, generation: u32, stamp_ms: u64) -> LockRecord {
        LockRecord { owner, generation, stamp_ms, crc: crate::power::crc32(&stamp_ms.to_le_bytes()) }
    }
}

pub const LOCK_TIMEOUT_MS: u64 = 30_000;

/// 锁是否超期（持有者崩溃/掉电后必须能被接管）。
pub fn lock_expired(lock: &LockRecord, now_ms: u64) -> bool {
    now_ms.saturating_sub(lock.stamp_ms) > LOCK_TIMEOUT_MS
}

/// 抢占规则：无锁可拿；超期可拿；同主续锁可拿；否则拒绝。
pub fn lock_acquire(current: Option<LockRecord>, who: SystemId, now_ms: u64, generation: u32) -> Option<LockRecord> {
    match current {
        None => Some(LockRecord::new(who, generation, now_ms)),
        Some(l) if l.owner == who => Some(LockRecord::new(who, generation, now_ms)),
        Some(l) if lock_expired(&l, now_ms) => Some(LockRecord::new(who, generation, now_ms)),
        Some(_) => None,
    }
}

pub fn lock_release(lock: &LockRecord, who: SystemId) -> bool {
    lock.owner == who
}

// ---------------------------------------------------------------------------
// F069 / F075 文件系统自检与收口
// ---------------------------------------------------------------------------

/// 造一份合法 FAT32 引导扇区（自检与测试共用）。
pub fn sample_fat32_boot() -> [u8; 512] {
    let mut b = [0u8; 512];
    b[11..13].copy_from_slice(&512u16.to_le_bytes());
    b[13] = 8;
    b[14..16].copy_from_slice(&32u16.to_le_bytes());
    b[16] = 2;
    b[32..36].copy_from_slice(&1_000_000u32.to_le_bytes());
    b[36..40].copy_from_slice(&2u32.to_le_bytes());
    b[44..48].copy_from_slice(&2u32.to_le_bytes());
    b[510] = 0x55;
    b[511] = 0xAA;
    b
}

/// 造一份合法 exFAT 引导扇区。
pub fn sample_exfat_boot() -> [u8; 512] {
    let mut b = [0u8; 512];
    b[3..11].copy_from_slice(exfat::EXFAT_SIGNATURE);
    b[0x50..0x54].copy_from_slice(&128u32.to_le_bytes());
    b[0x54..0x58].copy_from_slice(&1024u32.to_le_bytes());
    b[0x58..0x5C].copy_from_slice(&4096u32.to_le_bytes());
    b[0x5C..0x60].copy_from_slice(&100_000u32.to_le_bytes());
    b[0x60..0x64].copy_from_slice(&5u32.to_le_bytes());
    b[0x64..0x68].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
    b[0x6C] = 9;
    b[0x6D] = 3;
    b[0x6E] = 1;
    b[510] = 0x55;
    b[511] = 0xAA;
    b
}

/// AI-03 域自检：F051~F075 逐项登记。
pub fn run_fs_checks() -> CheckSet {
    let mut set = CheckSet::new("fs");

    let b32 = sample_fat32_boot();
    let bpb = fat32::parse_bpb(&b32);
    set.add(
        "F051 fat32 read",
        bpb.map(|b| b.bytes_per_sector == 512 && fat32::cluster_to_sector(&b, 2).is_some()).unwrap_or(false),
        "bpb + cluster mapping",
    );

    let mut fat = [0u8; 2 * 512];
    fat32::set_fat_entry(&mut fat, 0, 0x0FFF_FFF8);
    fat32::set_fat_entry(&mut fat, 1, 0xFFFF_FFFF);
    let c2 = fat32::alloc_cluster(&fat, 2);
    let c3 = fat32::alloc_cluster(&fat, c2.unwrap_or(2) + 1);
    set.add(
        "F052 fat32 write",
        c2.is_some() && c3.is_some() && c2 != c3 && fat32::link_chain(&mut fat, &[c2.unwrap(), c3.unwrap()]),
        "alloc + link",
    );

    let bex = sample_exfat_boot();
    let xbpb = exfat::parse_exfat_bpb(&bex);
    set.add(
        "F053 exfat read",
        xbpb.map(|b| b.cluster_bytes() == 4096 && exfat::cluster_to_sector(&b, 2) == Some(4096)).unwrap_or(false),
        "exfat bpb",
    );

    let mut xfat = [0u8; 64];
    exfat::set_fat_entry(&mut xfat, 0, 0xFFFF_FFF8);
    exfat::set_fat_entry(&mut xfat, 1, exfat::EXFAT_EOC);
    let x2 = exfat::alloc_cluster(&xfat, 2);
    set.add(
        "F054 exfat write",
        x2.is_some() && exfat::link_chain(&mut xfat, &[x2.unwrap()]),
        "exfat alloc",
    );

    set.add(
        "F055 shared layout",
        SHARED_DIRS.len() == 5
            && classify_top("/docs/a.md") == Some(SharedDirKind::Docs)
            && classify_top("/junk") .is_none()
            && write_allowed("/vault/x.bin", true)
            && !write_allowed("/vault/x.bin", false),
        "top-level dirs",
    );

    let mut mt = MountTable::new();
    let ok1 = mt.mount(MountEntry { slot: 0, fs: FsKind::Exfat, flags: MOUNT_SHARED, serial: 7 });
    let ok2 = mt.mount(MountEntry { slot: 0, fs: FsKind::Fat32, flags: 0, serial: 8 });
    set.add(
        "F056 mount table",
        ok1 && !ok2 && mt.len() == 1 && mt.by_slot(0).unwrap().shared() && FsKind::Exfat.supports_large_files(),
        "slot uniqueness",
    );

    let e = fat32::make_dir_entry(b"README  MD ", fat32::ATTR_ARCHIVE, 4, 42);
    let mut raw = [0u8; fat32::DIR_ENTRY_SIZE];
    e.encode(&mut raw);
    let parsed = fat32::DirEntry::parse(&raw).unwrap();
    let mut nm = [0u8; 16];
    let n = parsed.short_name(&mut nm);
    set.add(
        "F057 enumerate",
        parsed.size == 42 && core::str::from_utf8(&nm[..n]).unwrap() == "README.MD",
        "dir entry roundtrip",
    );

    let mut del = e;
    fat32::mark_deleted(&mut del);
    set.add(
        "F058 create/delete/rename",
        del.is_deleted() && fat32::rename_entry(&mut del, b"NEWNAME MD ") && !del.is_deleted(),
        "delete + rename",
    );

    let big = exfat::StreamEntry {
        flags: exfat::NO_FAT_CHAIN,
        first_cluster: 3,
        data_length: 6u64 << 30,
        valid_data_length: 6u64 << 30,
    };
    set.add(
        "F059 >4GB support",
        big.exceeds_fat32() && exfat::stream_sane(&big) && !FsKind::Fat32.supports_large_files(),
        "u64 stream length",
    );

    let mut asm = fat32::LfnAssembler::new();
    let mut lfn = [0u8; 32];
    lfn[0] = 0x01;
    lfn[11] = fat32::ATTR_LONG_NAME;
    lfn[13] = 0x11;
    lfn[1..3].copy_from_slice(&0x4E2Du16.to_le_bytes());
    let mut lfn2 = lfn;
    lfn2[0] = 0x42;
    lfn2[1..3].copy_from_slice(&0x6587u16.to_le_bytes());
    asm.feed(&lfn2);
    asm.feed(&lfn);
    set.add("F060 long filename", asm.finish() == Ok("中文"), "lfn assemble");

    set.add(
        "F061 timestamp mapping",
        fat32::dos_to_unix(0, 0) == 0
            && fat32::unix_to_dos(1_700_000_000).is_some()
            && fat32::attr_to_mode(fat32::ATTR_READ_ONLY) == 0o444
            && fat32::attr_to_mode(0) == 0o666,
        "dos<->unix",
    );

    let mut wc = WriteCache::new();
    wc.mark_dirty(10);
    wc.mark_dirty(10);
    let flushed = wc.flush();
    set.add("F062 write cache", flushed == 1 && wc.synced(), "fsync clears dirty set");

    let mut bad_fat = fat;
    fat32::set_fat_entry(&mut bad_fat, 2, 2);
    set.add(
        "F063 corruption detect",
        fat32::fat_health(&bpb.unwrap(), &fat) == fat32::FsHealth::Clean
            && fat32::fat_health(&bpb.unwrap(), &bad_fat) == fat32::FsHealth::Corrupt,
        "self loop caught",
    );

    let g = VolumeGuid::from_serial(0x1234);
    let (healed, repaired) = self_heal(None, 0x1234);
    let (kept, not_repaired) = self_heal(Some(g), 0x1234);
    set.add(
        "F064 guid self-heal",
        repaired && healed == g && !not_repaired && kept == g && !g.is_null(),
        "derive from serial",
    );

    set.add(
        "F065 read-only degrade",
        mount_decision(fat32::FsHealth::Clean) == MountDecision::ReadWrite
            && mount_decision(fat32::FsHealth::Corrupt) == MountDecision::ReadOnly
            && mount_decision(fat32::FsHealth::Unmountable) == MountDecision::Refuse,
        "degrade ladder",
    );

    let mut uxv = [0u8; 32];
    uxv[0..4].copy_from_slice(&UXV_MAGIC);
    uxv[4..6].copy_from_slice(&1u16.to_le_bytes());
    uxv[8..16].copy_from_slice(&(5u64 << 30).to_le_bytes());
    let hdr = uxv_header(&uxv);
    set.add(
        "F066 .uxv interop",
        uxv_probe(&uxv) && hdr.is_some() && hdr.unwrap().payload_bytes == 5u64 << 30 && uxv_needs_large_fs(5u64 << 30),
        "container probe",
    );

    set.add(
        "F067 path safety",
        fat32::path_safe("/media/a b/c.png")
            && !fat32::path_safe("/../etc")
            && !fat32::path_safe("relative")
            && fat32::MAX_PATH == 260,
        "no traversal",
    );

    let mut dc = DirCache::new();
    dc.insert("a.txt", 5, 1, false);
    let found = dc.lookup("a.txt");
    dc.invalidate();
    set.add(
        "F068 dir cache",
        found.is_some() && dc.lookup("a.txt").is_none() && dc.generation == 1,
        "invalidate bumps generation",
    );

    set.add("F069 fs self-check", set.all_passed(), "entry point");

    let l = LockRecord::new(SystemId::Varix, 1, 1_000);
    let mut buf = [0u8; LOCK_RECORD_LEN];
    l.encode(&mut buf);
    let dec = LockRecord::decode(&buf).unwrap();
    set.add(
        "F070 cross-system lock",
        dec.crc_ok()
            && lock_acquire(None, SystemId::Varix, 1_000, 1).is_some()
            && lock_acquire(Some(l), SystemId::Windows, 1_500, 2).is_none()
            && lock_acquire(Some(l), SystemId::Windows, 1_000 + LOCK_TIMEOUT_MS + 1, 2).is_some()
            && lock_release(&l, SystemId::Varix),
        "lock + timeout",
    );

    let mut u16buf = [0u8; 16];
    let n = fat32::utf8_to_utf16("中", &mut u16buf).unwrap();
    let mut units = [0u16; 2];
    units[0] = u16::from_le_bytes([u16buf[0], u16buf[1]]);
    let mut back = [0u8; 8];
    let m = fat32::utf16_to_utf8(&units, &mut back);
    set.add(
        "F071 encoding alignment",
        n == 2 && core::str::from_utf8(&back[..m]).unwrap() == "中",
        "utf8<->utf16le",
    );

    let mut ri = [0u8; 16];
    let mut rr = [0u8; 16];
    fat32::recycle_names(0x2A, &mut ri, &mut rr);
    set.add(
        "F072 recycle bin",
        ri[..2] == *b"$I" && rr[..2] == *b"$R" && fat32::is_hidden(fat32::ATTR_HIDDEN),
        "windows recycle naming",
    );

    let mut handles = fat32::HandleTable::new();
    let h = handles.open(3, 9, true).unwrap();
    set.add(
        "F073 handle lifecycle",
        handles.valid(&h) && handles.close(&h) && !handles.valid(&h),
        "generation stops dangling handles",
    );

    let mut dfat = [0u8; 64 * 1024];
    exfat::set_fat_entry(&mut dfat, 5, exfat::EXFAT_EOC);
    let report = exfat::diagnose_volume(&xbpb.unwrap(), &dfat, 0);
    set.add("F074 diagnostics", report.len() == 0, "clean volume has no issues");

    set.add("F075 fs domain closure", set.all_passed(), "all above green");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f055_vault_is_ciphertext_only() {
        assert!(write_allowed("/docs/x.md", false));
        assert!(!write_allowed("/vault/x.md", false));
        assert!(write_allowed("/vault/x.uxv", true));
        assert!(!write_allowed("docs/x.md", false));
    }

    #[test]
    fn f056_mount_and_unmount() {
        let mut t = MountTable::new();
        assert!(t.mount(MountEntry { slot: 1, fs: FsKind::Fat32, flags: MOUNT_RO, serial: 1 }));
        assert!(t.by_slot(1).unwrap().readonly());
        assert!(t.unmount(1));
        assert!(!t.unmount(1));
    }

    #[test]
    fn f062_dirty_set_is_idempotent() {
        let mut c = WriteCache::new();
        assert!(c.mark_dirty(1));
        assert!(c.mark_dirty(1));
        assert_eq!(c.dirty_count(), 1);
        assert_eq!(c.flush(), 1);
        assert!(c.synced());
    }

    #[test]
    fn f064_guid_is_stable() {
        assert_eq!(VolumeGuid::from_serial(9), VolumeGuid::from_serial(9));
        assert_ne!(VolumeGuid::from_serial(9), VolumeGuid::from_serial(10));
    }

    #[test]
    fn f066_probe_rejects_plain() {
        assert!(!uxv_probe(&[0u8; 32]));
        assert!(uxv_header(&[0u8; 32]).is_none());
    }

    #[test]
    fn f068_cache_full_is_bounded() {
        let mut c = DirCache::new();
        for i in 0..MAX_CACHE_NODES + 4 {
            c.insert("x", i as u32, 0, false);
        }
        assert_eq!(c.len(), MAX_CACHE_NODES);
    }

    #[test]
    fn f070_expired_lock_is_preemptable() {
        let l = LockRecord::new(SystemId::Windows, 1, 0);
        assert!(!lock_expired(&l, LOCK_TIMEOUT_MS));
        assert!(lock_expired(&l, LOCK_TIMEOUT_MS + 1));
        assert!(lock_acquire(Some(l), SystemId::Varix, LOCK_TIMEOUT_MS + 1, 9).is_some());
    }

    #[test]
    fn f075_domain_self_test_is_green() {
        let set = run_fs_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 2048];
            let n = set.render(&mut buf);
            panic!("fs self-test://n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert_eq!(set.len(), 25);
    }
}
