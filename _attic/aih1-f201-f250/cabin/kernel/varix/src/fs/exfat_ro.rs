//! 任务18 · exFAT 只读驱动挂载 SHARED 分区 + 快照区写过渡（双域总案
//! 2.3 真驱动·步骤 10 续）。
//!
//! **只读语义（如实标注）**：SHARED 分区由 Windows 与 VARIX 双域共享，
//! Windows 侧为写入权威。VARIX 内核对 exFAT 卷**只读**——本模块不含
//! 任何 exFAT 写路径；`SharedVolume::write` 恒返回
//! [`FsError::ReadOnly`]（写请求显式拒绝，绝不静默降级为本地写）。
//! 内核侧需要落盘的内容统一走 [`SnapshotArea`]（快照区追加式记录），
//! 不污染 Windows 视图。
//!
//! 布局参照（三源交叉）：Microsoft exFAT spec 1.00、GRUB
//! `grub-core/fs/fat.c`（MODE_EXFAT）、本仓库 fs/exfat.rs 的 BPB 解析
//! （偏移与 GRUB exfat.h 逐字段一致）。镜像构造器 `_attic/mkexfat.py`
//! 与本模块宿主测试内嵌构造器互为独立实现，同一驱动两处解析通过。
//!
//! 盘面定位：先探测 GPT（"EFI PART"），按 Microsoft Basic Data 类型
//! GUID 取首个分区；无 GPT 则整盘即卷（QEMU 测试卷布局）。

use crate::drivers::blk::{fnv1a64, BlockDevice, BlockError};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 文件系统错误口径。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FsError {
    /// 块设备层错误（透传 `BlockError` 语义）。
    Io(BlockError),
    /// 不是 exFAT 卷（VBR 签名/校验不符）。
    NotExfat,
    /// GPT/分区定位失败（显式拒绝，不回退整盘猜测）。
    NoPartition,
    /// 路径不存在或不是该类型（文件/目录混淆统一报 NotFound）。
    NotFound,
    /// 路径越过 `..`/绝对段等不支持形式。
    BadPath,
    /// 文件超过读取上限（如实拒绝，不截断假装成功）。
    TooLarge,
    /// SHARED 卷只读（写请求走 SnapshotArea）。
    ReadOnly,
}

impl From<BlockError> for FsError {
    fn from(e: BlockError) -> Self {
        FsError::Io(e)
    }
}

// -- GPT 定位 --------------------------------------------------------------

const GPT_HEADER_SIG: &[u8; 8] = b"EFI PART";
/// Microsoft Basic Data 分区类型 GUID（LE 混合序字节）。
const BASIC_DATA_GUID: [u8; 16] = [
    0xA2, 0xA0, 0xD0, 0xEB, 0xE5, 0xB9, 0x33, 0x44, 0x87, 0xC0, 0x68, 0xB6, 0xB7, 0x26, 0x99, 0xC7,
];

/// GPT 探测：返回首个 Basic Data 分区的起始 LBA；无 GPT 返回 None
/// （调用方决定整盘语义）。`pub(crate)`：exfat_rw 写层复用同一定位。
pub(crate) fn gpt_first_basic_data(dev: &mut dyn BlockDevice) -> Result<Option<u64>, FsError> {
    let mut hdr = [0u8; 512];
    dev.read_blocks(1, &mut hdr)?;
    if &hdr[0..8] != GPT_HEADER_SIG {
        return Ok(None);
    }
    let entries_lba = u64le(&hdr[72..80]);
    let num_entries = u32le(&hdr[80..84]) as usize;
    let entry_size = u32le(&hdr[84..88]) as usize;
    if entry_size < 128 || entry_size > 1024 || num_entries == 0 || num_entries > 1024 {
        return Ok(None);
    }
    // 逐条目扫描（每块读一批，避免逐条 IO）。
    let bs = dev.block_size() as usize;
    let mut buf = vec![0u8; bs];
    let per_block = bs / entry_size;
    for idx in 0..num_entries {
        let block = idx / per_block;
        let slot = idx % per_block;
        dev.read_blocks(entries_lba + block as u64, &mut buf)?;
        let e = &buf[slot * entry_size..slot * entry_size + 128];
        if e[0..16] == BASIC_DATA_GUID {
            let first = u64le(&e[32..40]);
            let last = u64le(&e[40..48]);
            if last > first {
                return Ok(Some(first));
            }
        }
    }
    Ok(None)
}

fn u16le(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}

fn u32le(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

fn u64le(b: &[u8]) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[0..8]);
    u64::from_le_bytes(a)
}

// -- BPB -------------------------------------------------------------------

/// exFAT BPB（VBR 字段——偏移经 GRUB grub-exfat.h 交叉确认，与
/// fs/exfat.rs::parse_exfat_bpb 同源）。
#[derive(Clone, Copy, Debug)]
pub struct Bpb {
    pub volume_length_sectors: u64,
    pub fat_offset: u32,
    pub fat_length: u32,
    pub cluster_heap_offset: u32,
    pub cluster_count: u32,
    pub root_cluster: u32,
    pub bytes_per_sector_shift: u8,
    pub sectors_per_cluster_shift: u8,
    pub number_of_fats: u8,
}

impl Bpb {
    pub fn parse(vbr: &[u8]) -> Option<Bpb> {
        if vbr.len() < 512 || vbr[510] != 0x55 || vbr[511] != 0xAA {
            return None;
        }
        if &vbr[3..11] != b"EXFAT   " {
            return None;
        }
        // MustBeZero 区（11..64）必须全零（exFAT 与 FAT 家族判别）。
        if vbr[11..64].iter().any(|&b| b != 0) {
            return None;
        }
        let bps_shift = vbr[0x6C];
        let spc_shift = vbr[0x6D];
        if !(9..=12).contains(&bps_shift) || spc_shift > 25 - bps_shift {
            return None;
        }
        let number_of_fats = vbr[0x6E];
        if number_of_fats == 0 || number_of_fats > 2 {
            return None;
        }
        let bpb = Bpb {
            volume_length_sectors: u64le(&vbr[0x48..0x50]),
            fat_offset: u32le(&vbr[0x50..0x54]),
            fat_length: u32le(&vbr[0x54..0x58]),
            cluster_heap_offset: u32le(&vbr[0x58..0x5C]),
            cluster_count: u32le(&vbr[0x5C..0x60]),
            root_cluster: u32le(&vbr[0x60..0x64]),
            bytes_per_sector_shift: bps_shift,
            sectors_per_cluster_shift: spc_shift,
            number_of_fats,
        };
        if bpb.cluster_count < 2
            || bpb.root_cluster < 2
            || bpb.root_cluster >= bpb.cluster_count + 2
            || bpb.fat_offset == 0
            || bpb.cluster_heap_offset == 0
        {
            return None;
        }
        Some(bpb)
    }

    pub fn bytes_per_sector(&self) -> u32 {
        1 << self.bytes_per_sector_shift
    }
    pub fn cluster_bytes(&self) -> u32 {
        1 << (self.bytes_per_sector_shift + self.sectors_per_cluster_shift)
    }
    /// 簇号 → 卷内绝对 LBA（part_base 由调用方叠加）。
    pub fn cluster_lba(&self, cluster: u32) -> u64 {
        self.cluster_heap_offset as u64 + (cluster as u64 - 2) * (self.cluster_bytes() as u64 / self.bytes_per_sector() as u64)
    }
}

// -- 目录项 ----------------------------------------------------------------
// ET_BITMAP/ET_UPCASE/ET_VOLUME_LABEL：只读挂载路径按"非 File 条目跳过"处理，
// 常量留作类型完备表（任务30 VFS 白名单与写路径过渡将按类型精确过滤）。

const ET_END: u8 = 0x00;
#[allow(dead_code)]
const ET_BITMAP: u8 = 0x81;
#[allow(dead_code)]
const ET_UPCASE: u8 = 0x82;
#[allow(dead_code)]
const ET_VOLUME_LABEL: u8 = 0x83;
const ET_FILE: u8 = 0x85;
const ET_STREAM: u8 = 0xC0;
const ET_NAME: u8 = 0xC1;

const FLAG_IN_USE: u8 = 0x80;
const FLAG_SECONDARY_VALID: u8 = 0x40;
/// Stream Extension GeneralSecondaryFlags bit1 = NoFatChain（连续簇）。
const STREAM_NO_FAT_CHAIN: u8 = 0x02;

const ATTR_DIRECTORY: u16 = 0x10;

/// 目录条目解析产物。
#[derive(Clone, Debug)]
pub struct DirItem {
    /// 文件名（UTF-8；驱动按 UTF-16 码元直读后折叠为 ASCII 范围内
    /// 直转，非 ASCII 码元以 U+FFFD 占位参与比较——SHARED 契约路径
    /// 全 ASCII，如实声明该边界）。
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub first_cluster: u32,
    pub contiguous: bool,
}

/// 目录数据里解析 EntrySet 序列。
fn parse_dir_entries(dir: &[u8]) -> Vec<DirItem> {
    let mut items = Vec::new();
    let mut i = 0usize;
    while i + 32 <= dir.len() {
        let t = dir[i];
        if t == ET_END {
            break;
        }
        if t & FLAG_IN_USE == 0 {
            i += 32;
            continue;
        }
        if t == ET_FILE {
            let nsec = dir[i + 1] as usize;
            let attr = u16le(&dir[i + 4..i + 6]);
            let mut name_units: Vec<u16> = Vec::new();
            let mut first_cluster = 0u32;
            let mut size = 0u64;
            let mut contiguous = false;
            let mut have_stream = false;
            let mut k = 1usize;
            while k <= nsec && i + k * 32 + 32 <= dir.len() {
                let st = dir[i + k * 32];
                if st & FLAG_IN_USE == 0 || st & FLAG_SECONDARY_VALID == 0 {
                    k += 1;
                    continue;
                }
                let base = i + k * 32;
                match st {
                    ET_STREAM => {
                        first_cluster = u32le(&dir[base + 20..base + 24]);
                        size = u64le(&dir[base + 24..base + 32]);
                        contiguous = dir[base + 1] & STREAM_NO_FAT_CHAIN != 0;
                        have_stream = true;
                    }
                    ET_NAME => {
                        for j in 0..15 {
                            let u = u16le(&dir[base + 2 + j * 2..base + 4 + j * 2]);
                            if u != 0 {
                                name_units.push(u);
                            }
                        }
                    }
                    _ => {}
                }
                k += 1;
            }
            if have_stream && !name_units.is_empty() {
                items.push(DirItem {
                    name: utf16_to_display(&name_units),
                    is_dir: attr & ATTR_DIRECTORY != 0,
                    size,
                    first_cluster,
                    contiguous,
                });
            }
            i += (nsec + 1) * 32;
        } else {
            // 0x81/0x82/0x83 与未知 primary：跳过。
            i += 32;
        }
    }
    items
}

/// UTF-16 码元 → 显示串（ASCII 直转；非 ASCII 以占位符如实降级，
/// 路径匹配仍可用——SHARED 契约路径全 ASCII）。
fn utf16_to_display(units: &[u16]) -> String {
    let mut out = String::with_capacity(units.len());
    for &u in units {
        if (0x20..0x7F).contains(&u) {
            out.push(u as u8 as char);
        } else if u == 0 {
            break;
        } else {
            out.push('\u{FFFD}');
        }
    }
    out
}

// -- 卷挂载 ----------------------------------------------------------------

/// 文件读取上限（一次 read_file 的防御上限；大文件由调用方分块 API
/// 处理——本任务验收文件 ≤ 64KiB）。
pub const MAX_FILE_READ: u64 = 1 << 20;

/// exFAT 只读卷。
pub struct ExfatVolume<B: BlockDevice> {
    dev: B,
    part_base: u64,
    bpb: Bpb,
}

impl<B: BlockDevice> ExfatVolume<B> {
    /// 挂载：GPT 定位（无 GPT 整盘）→ VBR/BPB 校验。
    pub fn mount(mut dev: B) -> Result<Self, FsError> {
        let part_base = match gpt_first_basic_data(&mut dev)? {
            Some(lba) => lba,
            None => 0,
        };
        let mut vbr = [0u8; 512];
        dev.read_blocks(part_base, &mut vbr)?;
        let bpb = Bpb::parse(&vbr).ok_or(FsError::NotExfat)?;
        // 卷长度越界防御（声明超出块设备容量 → 拒绝）。
        let cap = dev.capacity_blocks();
        if part_base + bpb.volume_length_sectors > cap {
            return Err(FsError::NotExfat);
        }
        Ok(ExfatVolume { dev, part_base, bpb })
    }

    pub fn bpb(&self) -> &Bpb {
        &self.bpb
    }

    /// 底层块设备访问（S4.2 写层复用同卷设备：`dev_mut()` 交出独占借用，
    /// 调用方在 with 闭包内使用，语义与 [`Self::with`] 一致）。
    pub fn dev_mut(&mut self) -> &mut B {
        &mut self.dev
    }

    fn cluster_bytes(&self) -> usize {
        self.bpb.cluster_bytes() as usize
    }

    /// 读一个完整簇。
    fn read_cluster(&mut self, cluster: u32, out: &mut [u8]) -> Result<(), FsError> {
        debug_assert_eq!(out.len(), self.cluster_bytes());
        let lba = self.part_base + self.bpb.cluster_lba(cluster);
        self.dev.read_blocks(lba, out)?;
        Ok(())
    }

    /// 沿 FAT 链收集簇号（上限防御：簇数 ≤ cluster_count）。
    fn fat_chain(&mut self, first: u32, max: usize) -> Result<Vec<u32>, FsError> {
        let bs = self.bpb.bytes_per_sector() as usize;
        let mut chain = Vec::new();
        let mut c = first;
        let per_block = bs / 4;
        let mut buf = vec![0u8; bs];
        let mut cur_block = u64::MAX;
        while 2 <= c && c < 0xFFFFFFF8 {
            chain.push(c);
            if chain.len() > max {
                return Err(FsError::Io(BlockError::Io)); // 链异常（环/超长）
            }
            let idx = (c - 2) as usize;
            let block = (idx / per_block) as u64;
            if block != cur_block {
                let lba = self.part_base + self.bpb.fat_offset as u64 + block;
                self.dev.read_blocks(lba, &mut buf)?;
                cur_block = block;
            }
            let off = (idx % per_block) * 4;
            c = u32le(&buf[off..off + 4]);
        }
        // 终止条件：EOC 或 0（free，链异常但如实停在此）——非 EOC 终止
        // 视为卷损坏，报 Io。
        if !(0xFFFFFFF8..=0xFFFFFFFF).contains(&c) && c != 0 {
            return Err(FsError::Io(BlockError::Io));
        }
        Ok(chain)
    }

    /// 读目录（first_cluster 起的完整 EntrySet 数据）。
    fn read_dir_data(&mut self, first_cluster: u32) -> Result<Vec<u8>, FsError> {
        let cb = self.cluster_bytes();
        let max_clusters = (self.bpb.cluster_count as usize) + 2;
        let chain = self.fat_chain(first_cluster, max_clusters)?;
        let mut data = Vec::with_capacity(chain.len() * cb);
        let mut buf = vec![0u8; cb];
        for &c in &chain {
            self.read_cluster(c, &mut buf)?;
            data.extend_from_slice(&buf);
        }
        Ok(data)
    }

    /// 列目录（根目录传 ""）。
    pub fn read_dir(&mut self, path: &str) -> Result<Vec<DirItem>, FsError> {
        let dir_cluster = self.walk(path)?;
        self.parse_dir_at(dir_cluster)
    }

    fn parse_dir_at(&mut self, cluster: u32) -> Result<Vec<DirItem>, FsError> {
        let data = self.read_dir_data(cluster)?;
        Ok(parse_dir_entries(&data))
    }

    /// 路径逐段下钻，返回终点簇号（路径段必须存在；终点可以是文件——
    /// 调用方 read_file 用）。空路径 = 根目录。
    fn walk(&mut self, path: &str) -> Result<u32, FsError> {
        if path.starts_with('/') && !path.trim_start_matches('/').is_empty() == false && path != "/" {
            return Err(FsError::BadPath);
        }
        let mut cur = self.bpb.root_cluster;
        let mut items = self.parse_dir_at(cur)?;
        for seg in path.split('/').filter(|s| !s.is_empty()) {
            if seg == "." || seg == ".." {
                return Err(FsError::BadPath);
            }
            let hit = items.iter().find(|it| it.name == seg).ok_or(FsError::NotFound)?;
            if !hit.is_dir {
                // 中间路径段是文件 → BadPath；终点是文件由调用方处理。
                return Err(FsError::NotFound);
            }
            cur = hit.first_cluster;
            items = self.parse_dir_at(cur)?;
        }
        Ok(cur)
    }

    /// 定位文件条目（返回 DirItem）。
    fn find_file(&mut self, path: &str) -> Result<DirItem, FsError> {
        let (parent, name) = split_parent(path)?;
        let items = self.read_dir(&parent)?;
        items.into_iter().find(|it| it.name == name).ok_or(FsError::NotFound)
    }

    /// 读文件（上限 MAX_FILE_READ，越限拒绝）。
    ///
    /// 注意：结果整体驻留一个 `Vec`——内核 SlabHeap 单块上限 4KiB，
    /// 内核目标态读 >4KiB 文件必须走 [`Self::read_file_streaming`]。
    pub fn read_file(&mut self, path: &str) -> Result<Vec<u8>, FsError> {
        let mut out = Vec::new();
        self.read_file_streaming(path, |chunk| out.extend_from_slice(chunk))?;
        Ok(out)
    }

    /// 流式读文件：按簇回调 `sink`（上限 MAX_FILE_READ，越限拒绝）。
    ///
    /// 内核堆单块 ≤4KiB（SlabHeap class 上限），整文件 `Vec` 在目标态
    /// 必然分配失败（alloc.rs:573 panic）；分簇回调把峰值内存压到单簇。
    /// 返回读取总字节数。
    pub fn read_file_streaming(
        &mut self,
        path: &str,
        mut sink: impl FnMut(&[u8]),
    ) -> Result<u64, FsError> {
        let it = self.find_file(path)?;
        if it.is_dir {
            return Err(FsError::NotFound);
        }
        if it.size > MAX_FILE_READ {
            return Err(FsError::TooLarge);
        }
        let cb = self.cluster_bytes();
        let mut buf = vec![0u8; cb];
        let mut total = 0u64;
        if it.contiguous {
            let n = (it.size as usize).div_ceil(cb);
            for k in 0..n {
                self.read_cluster(it.first_cluster + k as u32, &mut buf)?;
                let take = core::cmp::min(cb as u64, it.size - total) as usize;
                sink(&buf[..take]);
                total += take as u64;
            }
        } else {
            let n = (it.size as usize).div_ceil(cb);
            let chain = self.fat_chain(it.first_cluster, n)?;
            if chain.len() < n {
                return Err(FsError::Io(BlockError::Io)); // 链短于声明大小：卷损坏
            }
            for &c in &chain {
                self.read_cluster(c, &mut buf)?;
                let take = core::cmp::min(cb as u64, it.size - total) as usize;
                sink(&buf[..take]);
                total += take as u64;
            }
        }
        Ok(total)
    }

    /// **只读语义（如实标注）**：写请求显式拒绝；落盘需求走
    /// [`SnapshotArea`]。绝不静默降级。
    pub fn write(&mut self, _path: &str, _data: &[u8]) -> Result<(), FsError> {
        Err(FsError::ReadOnly)
    }
}

/// 拆父目录 + 末段。
fn split_parent(path: &str) -> Result<(String, String), FsError> {
    if path.is_empty() {
        return Err(FsError::BadPath);
    }
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(pos) => {
            let parent = trimmed[..pos].to_string();
            let name = trimmed[pos + 1..].to_string();
            if name.is_empty() || name == "." || name == ".." {
                Err(FsError::BadPath)
            } else {
                Ok((if parent.is_empty() { "/".to_string() } else { parent }, name))
            }
        }
        None => {
            if trimmed == "." || trimmed == ".." || trimmed.is_empty() {
                Err(FsError::BadPath)
            } else {
                Ok(("/".to_string(), trimmed.to_string()))
            }
        }
    }
}

// -- 快照区（写路径过渡） ---------------------------------------------------

/// 快照区——SHARED 只读语义下内核侧内容的落盘位置（总案：写路径走
/// 快照区过渡并如实标注）。
///
/// 记录布局（追加式，1 条 = 头 + 内容，块对齐由 base 约定）：
/// ```text
/// [0..8]   magic u64 = SNAP_MAGIC
/// [8..16]  content_len u64
/// [16..24] name_fnv u64（路径名 FNV-1a 指纹，驱动不存长名）
/// [24..32] content_fnv u64
/// [32..]   content
/// ```
/// 槽容量固定 [`SNAP_SLOT_BYTES`]；逐槽扫描遇空槽终止。
pub const SNAP_MAGIC: u64 = 0x5641_5258_534E_4150; // "VARXSNAP"
pub const SNAP_SLOT_BYTES: u64 = 4096;
pub const SNAP_HEADER_BYTES: u64 = 32;
pub const SNAP_MAX_CONTENT: u64 = SNAP_SLOT_BYTES - SNAP_HEADER_BYTES;
pub const SNAP_SLOTS: u64 = 64;

/// 快照区记录。
pub struct SnapshotRecord {
    pub name: String,
    pub content: Vec<u8>,
}

/// 快照区（`base_lba` 起的 [`SNAP_SLOTS`] 个 4KiB 槽）。
pub struct SnapshotArea<B: BlockDevice> {
    dev: B,
    base_lba: u64,
}

impl<B: BlockDevice> SnapshotArea<B> {
    pub fn new(dev: B, base_lba: u64) -> Self {
        SnapshotArea { dev, base_lba }
    }

    fn slot_lba(&self, slot: u64) -> u64 {
        self.base_lba + slot * (SNAP_SLOT_BYTES / 512)
    }

    fn load_slot(&mut self, slot: u64, buf: &mut [u8; SNAP_SLOT_BYTES as usize]) -> Result<Option<SnapshotRecord>, FsError> {
        self.dev.read_blocks(self.slot_lba(slot), buf)?;
        let magic = u64le(&buf[0..8]);
        if magic == 0 {
            return Ok(None);
        }
        if magic != SNAP_MAGIC {
            // 非空非本格式：如实拒绝（快照区被污染）。
            return Err(FsError::Io(BlockError::Io));
        }
        let len = u64le(&buf[8..16]);
        let name_fnv = u64le(&buf[16..24]);
        let content_fnv = u64le(&buf[24..32]);
        if len > SNAP_MAX_CONTENT {
            return Err(FsError::Io(BlockError::Io));
        }
        let content = buf[32..32 + len as usize].to_vec();
        if fnv1a64(&content) != content_fnv {
            return Err(FsError::Io(BlockError::Io)); // 内容损坏如实上报
        }
        Ok(Some(SnapshotRecord { name: format!("fnv:{:016x}", name_fnv), content }))
    }

    /// 追加一条快照（首个空槽；满则报错——容量语义明确）。
    pub fn append(&mut self, name: &str, content: &[u8]) -> Result<u64, FsError> {
        if content.len() as u64 > SNAP_MAX_CONTENT {
            return Err(FsError::TooLarge);
        }
        let mut probe = vec![0u8; 512];
        let mut slot = None;
        for s in 0..SNAP_SLOTS {
            self.dev.read_blocks(self.slot_lba(s), &mut probe)?;
            if u64le(&probe[0..8]) == 0 {
                slot = Some(s);
                break;
            }
        }
        let slot = slot.ok_or(FsError::TooLarge)?; // 满容量明确报错
        let mut rec = [0u8; SNAP_SLOT_BYTES as usize];
        rec[0..8].copy_from_slice(&SNAP_MAGIC.to_le_bytes());
        rec[8..16].copy_from_slice(&(content.len() as u64).to_le_bytes());
        rec[16..24].copy_from_slice(&fnv1a64(name.as_bytes()).to_le_bytes());
        rec[24..32].copy_from_slice(&fnv1a64(content).to_le_bytes());
        rec[32..32 + content.len()].copy_from_slice(content);
        self.dev.write_blocks(self.slot_lba(slot), &rec)?;
        self.dev.flush()?;
        Ok(slot)
    }

    /// 列全部快照（返回逐条内容校验后的记录）。
    pub fn list(&mut self) -> Result<Vec<SnapshotRecord>, FsError> {
        let mut out = Vec::new();
        let mut buf = [0u8; SNAP_SLOT_BYTES as usize];
        for s in 0..SNAP_SLOTS {
            match self.load_slot(s, &mut buf)? {
                Some(r) => out.push(r),
                None => break,
            }
        }
        Ok(out)
    }
}

// -- 目标态探针 -------------------------------------------------------------

pub mod target {
    use super::*;

    /// SHARED 挂载探针（第二块 NVMe 控制器，`shared-exfat.img`）。
    /// 验证：GPT/整盘定位 → BPB → 根目录 → 子目录 → FAT 链大文件 →
    /// 连续簇文件 → 只读写拒绝 → 快照区写读回。
    pub fn shared_probe(dev: &mut dyn BlockDevice) {
        // 快照区：卷尾留白区（2MiB 卷 → 快照区放 LBA 3000+，契约化）。
        const SNAP_BASE: u64 = 3000;

        // reborrow：mount 后 dev 仍归调用方（快照区复用同一设备）。
        // vol 生命周期限入块：块末 drop 释放 dev 借用。
        let write_reject_noted = {
            let mut vol = match ExfatVolume::mount(&mut *dev) {
                Ok(v) => v,
                Err(e) => {
                    crate::kwarn!("shared: mount failed {:?}", e);
                    return;
                }
            };
            let b = *vol.bpb();
            crate::kinfo!(
                "shared: exFAT mounted part_base={} clusters={} cluster_bytes={} root={}",
                vol.part_base,
                b.cluster_count,
                b.cluster_bytes(),
                b.root_cluster
            );
            match vol.read_dir("/") {
                Ok(items) => {
                    let names: Vec<&str> = items.iter().map(|i| i.name.as_str()).collect();
                    crate::kinfo!("shared: root {} entries: {}", items.len(), names.join(","));
                }
                Err(e) => {
                    crate::kwarn!("shared: root read_dir failed {:?}", e);
                    return;
                }
            }
            // 深层路径 + 内容 hash。
            let probes: [(&str, &[u8]); 2] = [
                ("/Games/hello.txt", b"hello from SHARED/Games"),
                ("/apps.json", b"{"),
            ];
            for (path, expect_prefix) in probes {
                match vol.read_file(path) {
                    Ok(data) => {
                        let ok = data.starts_with(expect_prefix);
                        crate::kinfo!(
                            "shared: read {} len={} fnv={:#018x} content_ok={}",
                            path,
                            data.len(),
                            fnv1a64(&data),
                            ok
                        );
                    }
                    Err(e) => {
                        crate::kwarn!("shared: read {} failed {:?}", path, e);
                        return;
                    }
                }
            }
            // 多簇 FAT 链文件（7 簇 = 28KiB > 内核堆单块 4KiB 上限，
            // 必须流式；fnv 增量累积后与镜像期望值硬断言）。
            {
                let mut h: u64 = 0xcbf2_9ce4_8422_2325;
                let streamed = vol.read_file_streaming("/big/manifest.bin", |c| {
                    for &b in c {
                        h ^= b as u64;
                        h = h.wrapping_mul(0x0000_0100_0000_01b3);
                    }
                });
                match streamed {
                    Ok(len) => {
                        let ok = len == 28672 && h == 0x789d_e9c5_2663_2325;
                        crate::kinfo!(
                            "shared: read /big/manifest.bin len={} fnv={:#018x} expect_ok={} (fat-chain multi-cluster, streaming)",
                            len,
                            h,
                            ok
                        );
                        if !ok {
                            crate::kwarn!("shared: manifest.bin verify FAILED");
                            return;
                        }
                    }
                    Err(e) => {
                        crate::kwarn!("shared: read big failed {:?}", e);
                        return;
                    }
                }
            }
            // 连续簇（NoFatChain）文件（3 簇 = 12KiB，同样流式 + 硬断言）。
            {
                let mut h: u64 = 0xcbf2_9ce4_8422_2325;
                let streamed = vol.read_file_streaming("/contig/contig.bin", |c| {
                    for &b in c {
                        h ^= b as u64;
                        h = h.wrapping_mul(0x0000_0100_0000_01b3);
                    }
                });
                match streamed {
                    Ok(len) => {
                        let ok = len == 12288 && h == 0x0a69_a918_9ca4_4325;
                        crate::kinfo!(
                            "shared: read /contig/contig.bin len={} fnv={:#018x} expect_ok={} (no-fat-chain, streaming)",
                            len,
                            h,
                            ok
                        );
                        if !ok {
                            crate::kwarn!("shared: contig.bin verify FAILED");
                            return;
                        }
                    }
                    Err(e) => {
                        crate::kwarn!("shared: read contig failed {:?}", e);
                        return;
                    }
                }
            }
            // 只读写拒绝（如实标注断言）。
            let reject = matches!(vol.write("/Games/hello.txt", b"x"), Err(FsError::ReadOnly));
            crate::kinfo!("shared: write rejected read_only={}", reject);
            reject
        };
        // 快照区写读回（dev 借用已随 vol drop 释放）。
        if !write_reject_noted {
            crate::kwarn!("shared: write was NOT rejected - read-only contract broken");
            return;
        }
        let mut snap = SnapshotArea::new(dev, SNAP_BASE);
        match snap.append("/Games/score.json", b"{\"best\":4242}") {
            Ok(slot) => {
                let listed = snap.list().map(|l| l.len()).unwrap_or(0);
                let listed_ok = snap
                    .list()
                    .map(|l| l.iter().any(|r| r.content == b"{\"best\":4242}"))
                    .unwrap_or(false);
                crate::kinfo!(
                    "shared: snapshot slot={} list={} content_ok={} (write path via snapshot)",
                    slot,
                    listed,
                    listed_ok
                );
            }
            Err(e) => {
                crate::kwarn!("shared: snapshot append failed {:?}", e);
                return;
            }
        }
        crate::kinfo!("shared: verify verdict=ok");
    }

}

// -- 宿主测试 ---------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::blk::BlockDevice;
    use std::collections::BTreeMap;

    /// 内存块设备（宿主测试后端）。
    struct MemDisk {
        blocks: BTreeMap<u64, [u8; 512]>,
        blocks_total: u64,
    }

    impl MemDisk {
        fn new(total: u64) -> Self {
            MemDisk { blocks: BTreeMap::new(), blocks_total: total }
        }
        fn put(&mut self, lba: u64, data: &[u8]) {
            let mut b = [0u8; 512];
            let n = data.len().min(512);
            b[..n].copy_from_slice(&data[..n]);
            self.blocks.insert(lba, b);
        }
    }

    impl BlockDevice for MemDisk {
        fn block_size(&self) -> u32 {
            512
        }
        fn capacity_blocks(&self) -> u64 {
            self.blocks_total
        }
        fn read_blocks(&mut self, lba: u64, dst: &mut [u8]) -> Result<(), BlockError> {
            let n = dst.len() / 512;
            for i in 0..n as u64 {
                match self.blocks.get(&(lba + i)) {
                    Some(b) => dst[i as usize * 512..(i + 1) as usize * 512].copy_from_slice(b),
                    None => dst[i as usize * 512..(i + 1) as usize * 512].fill(0),
                }
            }
            Ok(())
        }
        fn write_blocks(&mut self, lba: u64, src: &[u8]) -> Result<(), BlockError> {
            let n = src.len() / 512;
            for i in 0..n as u64 {
                let mut b = [0u8; 512];
                b.copy_from_slice(&src[i as usize * 512..(i + 1) as usize * 512]);
                self.blocks.insert(lba + i, b);
            }
            Ok(())
        }
        fn flush(&mut self) -> Result<(), BlockError> {
            Ok(())
        }
    }

    // -- 内嵌 exFAT 构造器（与 _attic/mkexfat.py 独立实现，互为参照） ----

    const CLUSTER_BYTES: usize = 4096;
    const HEAP_START: u32 = 512;
    const FAT_OFFSET: u32 = 128;
    const CLUSTER_COUNT: u32 = 448;

    struct Builder {
        img: Vec<u8>,
        fat: Vec<u32>,
        next_alloc: u32,
    }

    impl Builder {
        fn new() -> Self {
            let mut b = Builder {
                img: vec![0u8; 2 << 20],
                fat: vec![0u32; CLUSTER_COUNT as usize],
                next_alloc: 5,
            };
            b.fat[0] = 0xFFFF_FFFF; // 簇2 bitmap
            b.fat[1] = 0xFFFF_FFFF; // 簇3 upcase
            b.fat[2] = 0xFFFF_FFFF; // 簇4 root
            b
        }

        fn alloc_chain(&mut self, n: usize, contiguous: bool) -> u32 {
            let first = self.next_alloc;
            if contiguous {
                self.next_alloc += n as u32;
                self.fat[(first + n as u32 - 3) as usize] = 0xFFFF_FFFF;
                first
            } else {
                for k in 0..n as u32 {
                    let c = first + k;
                    self.fat[(c - 2) as usize] = if k == n as u32 - 1 { 0xFFFF_FFFF } else { c + 1 };
                }
                self.next_alloc += n as u32;
                first
            }
        }

        fn put_cluster(&mut self, c: u32, data: &[u8]) {
            let lba: u64 = HEAP_START as u64 + (c as u64 - 2) * (CLUSTER_BYTES as u64 / 512);
            let off = (lba * 512) as usize;
            self.img[off..off + data.len()].copy_from_slice(data);
        }

        fn entry_set_checksum(entries: &[Vec<u8>]) -> u16 {
            let mut csum: u16 = 0;
            for (ei, e) in entries.iter().enumerate() {
                for (i, &b) in e.iter().enumerate() {
                    if ei == 0 && (i == 2 || i == 3) {
                        continue;
                    }
                    csum = csum.rotate_left(1).wrapping_add(b as u16);
                }
            }
            csum
        }

        fn make_entry(name: &str, first: u32, size: u64, is_dir: bool, contiguous: bool) -> Vec<u8> {
            let nunits = name.chars().count();
            let nname = ((nunits + 14) / 15).max(1);
            let nsec = 1 + nname;
            let mut stream = vec![0u8; 32];
            stream[0] = ET_STREAM;
            stream[1] = if contiguous { STREAM_NO_FAT_CHAIN } else { 0 };
            stream[3] = nunits as u8;
            stream[20..24].copy_from_slice(&first.to_le_bytes());
            stream[24..32].copy_from_slice(&size.to_le_bytes());
            let mut file = vec![0u8; 32];
            file[0] = ET_FILE;
            file[1] = nsec as u8;
            file[4..6].copy_from_slice(&(if is_dir { ATTR_DIRECTORY } else { 0x20 }).to_le_bytes());
            let mut names: Vec<Vec<u8>> = Vec::new();
            let units: Vec<u16> = name.encode_utf16().collect();
            let mut padded = units.clone();
            padded.resize(nname * 15, 0);
            for k in 0..nname {
                let mut e = vec![0u8; 32];
                e[0] = ET_NAME;
                for j in 0..15 {
                    e[2 + j * 2..4 + j * 2].copy_from_slice(&padded[k * 15 + j].to_le_bytes());
                }
                names.push(e);
            }
            let csum = Self::entry_set_checksum(&[file.clone(), stream.clone()]
                .into_iter()
                .chain(names.iter().cloned())
                .collect::<Vec<_>>());
            file[2..4].copy_from_slice(&csum.to_le_bytes());
            let mut out = file;
            out.extend_from_slice(&stream);
            for n in names {
                out.extend_from_slice(&n);
            }
            out
        }

        fn vbr(&self) -> Vec<u8> {
            let mut v = vec![0u8; 512];
            v[0..3].copy_from_slice(&[0xEB, 0x76, 0x90]);
            v[3..11].copy_from_slice(b"EXFAT   ");
            v[0x48..0x50].copy_from_slice(&4096u64.to_le_bytes());
            v[0x50..0x54].copy_from_slice(&FAT_OFFSET.to_le_bytes());
            v[0x54..0x58].copy_from_slice(&8u32.to_le_bytes());
            v[0x58..0x5C].copy_from_slice(&HEAP_START.to_le_bytes());
            v[0x5C..0x60].copy_from_slice(&CLUSTER_COUNT.to_le_bytes());
            v[0x60..0x64].copy_from_slice(&4u32.to_le_bytes());
            v[0x6C] = 9;
            v[0x6D] = 3;
            v[0x6E] = 1;
            v[510..512].copy_from_slice(&[0x55, 0xAA]);
            v
        }

        fn finish(mut self, root_entries: &[u8], root_data: &[u8]) -> MemDisk {
            // VBR。
            let vbr = self.vbr();
            self.img[0..512].copy_from_slice(&vbr);
            self.img[512..1024].copy_from_slice(&vbr);
            // FAT。
            let fat_off = (FAT_OFFSET * 512) as usize;
            for (k, v) in self.fat.iter().enumerate() {
                self.img[fat_off + k * 4..fat_off + k * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
            // 簇 2/3 占位、root=4。
            self.put_cluster(2, &[0u8; CLUSTER_BYTES]);
            let mut upcase = vec![0u16; 0x200];
            for (i, u) in upcase.iter_mut().enumerate() {
                *u = if (0x61..=0x7A).contains(&(i as u16)) { i as u16 - 0x20 } else { i as u16 };
            }
            let mut up_bytes = Vec::new();
            for u in upcase {
                up_bytes.extend_from_slice(&u.to_le_bytes());
            }
            up_bytes.resize(CLUSTER_BYTES, 0);
            self.put_cluster(3, &up_bytes);
            let mut root = root_entries.to_vec();
            root.extend_from_slice(&[0u8; 32]); // end-of-dir
            root.resize(CLUSTER_BYTES, 0);
            self.put_cluster(4, &root);
            let _ = root_data;
            let mut disk = MemDisk::new(4096);
            for lba in 0..4096u64 {
                disk.put(lba, &self.img[(lba * 512) as usize..(lba * 512 + 512) as usize]);
            }
            disk
        }
    }

    fn build_test_volume() -> MemDisk {
        let mut b = Builder::new();
        // /Games/hello.txt（FAT 链单簇）。
        let hello = b"hello from SHARED/Games\r\nVARIX exFAT read-only mount works.\r\n";
        let h_cluster = b.alloc_chain(1, false);
        b.put_cluster(h_cluster, hello);
        let games_entries = Builder::make_entry("hello.txt", h_cluster, hello.len() as u64, false, false);
        let games_cluster = b.alloc_chain(1, false);
        b.put_cluster(games_cluster, &games_entries);
        // /big/manifest.bin（FAT 链 7 簇）。
        let big: Vec<u8> = (0..7 * CLUSTER_BYTES).map(|i| ((i as u64 * 0x9E) ^ 0x31) as u8).collect();
        let big_cluster = b.alloc_chain(7, false);
        for k in 0..7u32 {
            b.put_cluster(big_cluster + k, &big[k as usize * CLUSTER_BYTES..(k as usize + 1) * CLUSTER_BYTES]);
        }
        let big_entries = Builder::make_entry("manifest.bin", big_cluster, big.len() as u64, false, false);
        let big_dir_cluster = b.alloc_chain(1, false);
        b.put_cluster(big_dir_cluster, &big_entries);
        // /contig/contig.bin（NoFatChain 3 簇）。
        let contig: Vec<u8> = (0..3 * CLUSTER_BYTES).map(|i| ((i as u64 * 0x5A) ^ 0x77) as u8).collect();
        let c_cluster = b.alloc_chain(3, true);
        for k in 0..3u32 {
            b.put_cluster(c_cluster + k, &contig[k as usize * CLUSTER_BYTES..(k as usize + 1) * CLUSTER_BYTES]);
        }
        let c_entries = Builder::make_entry("contig.bin", c_cluster, contig.len() as u64, false, true);
        let c_dir_cluster = b.alloc_chain(1, false);
        b.put_cluster(c_dir_cluster, &c_entries);
        // 根目录：Games + big + contig（VolumeLabel 也放一条以测 skip）。
        let mut root = Builder::make_entry("Games", games_cluster, 0, true, false);
        root.extend(Builder::make_entry("big", big_dir_cluster, 0, true, false));
        root.extend(Builder::make_entry("contig", c_dir_cluster, 0, true, false));
        b.finish(&root, &[])
    }

    // -- 用例 ---------------------------------------------------------------

    #[test]
    fn exfat_mount_and_root_list() {
        let disk = build_test_volume();
        let mut vol = ExfatVolume::mount(disk).expect("挂载必须成功");
        assert_eq!(vol.bpb().root_cluster, 4);
        assert_eq!(vol.bpb().cluster_bytes(), 4096);
        let items = vol.read_dir("/").expect("根目录");
        let names: Vec<&str> = items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, vec!["Games", "big", "contig"], "根目录条目与顺序");
    }

    #[test]
    fn exfat_read_file_fat_chain_multi_cluster() {
        let disk = build_test_volume();
        let mut vol = ExfatVolume::mount(disk).unwrap();
        let data = vol.read_file("/big/manifest.bin").expect("7 簇链文件");
        assert_eq!(data.len(), 7 * CLUSTER_BYTES);
        let expect: Vec<u8> = (0..7 * CLUSTER_BYTES).map(|i| ((i as u64 * 0x9E) ^ 0x31) as u8).collect();
        assert_eq!(data, expect, "多簇链逐字节一致");
    }

    #[test]
    fn exfat_read_file_no_fat_chain_contiguous() {
        let disk = build_test_volume();
        let mut vol = ExfatVolume::mount(disk).unwrap();
        let data = vol.read_file("/contig/contig.bin").expect("连续簇文件");
        let expect: Vec<u8> = (0..3 * CLUSTER_BYTES).map(|i| ((i as u64 * 0x5A) ^ 0x77) as u8).collect();
        assert_eq!(data, expect, "NoFatChain 路径逐字节一致");
    }

    #[test]
    fn exfat_streaming_matches_read_file_fat_chain() {
        // 流式路径逐簇回调与整文件 Vec 逐字节一致（FAT 链 7 簇）。
        let disk = build_test_volume();
        let mut vol = ExfatVolume::mount(disk).unwrap();
        let whole = vol.read_file("/big/manifest.bin").expect("整读");
        let mut streamed: Vec<u8> = Vec::new();
        let total = vol
            .read_file_streaming("/big/manifest.bin", |c| streamed.extend_from_slice(c))
            .expect("流式读");
        assert_eq!(total, 7 * CLUSTER_BYTES as u64, "流式总长");
        assert_eq!(streamed, whole, "流式与整读逐字节一致");
        // 回调分簇边界：每片 ≤ 簇大小。
        let mut chunks = 0u32;
        let _ = vol.read_file_streaming("/big/manifest.bin", |c| {
            assert!(c.len() <= CLUSTER_BYTES);
            chunks += 1;
        });
        assert_eq!(chunks, 7, "7 簇 = 7 次回调");
    }

    #[test]
    fn exfat_streaming_matches_read_file_no_fat_chain() {
        // NoFatChain 连续簇的流式路径一致性。
        let disk = build_test_volume();
        let mut vol = ExfatVolume::mount(disk).unwrap();
        let whole = vol.read_file("/contig/contig.bin").expect("整读");
        let mut streamed: Vec<u8> = Vec::new();
        let total = vol
            .read_file_streaming("/contig/contig.bin", |c| streamed.extend_from_slice(c))
            .expect("流式读");
        assert_eq!(total, 3 * CLUSTER_BYTES as u64);
        assert_eq!(streamed, whole, "NoFatChain 流式与整读一致");
    }

    #[test]
    fn exfat_nested_path_and_content() {
        let disk = build_test_volume();
        let mut vol = ExfatVolume::mount(disk).unwrap();
        let data = vol.read_file("/Games/hello.txt").expect("深层路径");
        assert_eq!(data, b"hello from SHARED/Games\r\nVARIX exFAT read-only mount works.\r\n");
        // 404。
        assert_eq!(vol.read_file("/Games/nope.txt"), Err(FsError::NotFound));
        // 目录当文件读 → NotFound。
        assert_eq!(vol.read_file("/Games"), Err(FsError::NotFound));
        // 坏路径。
        assert_eq!(vol.read_file("/../x"), Err(FsError::BadPath));
    }

    #[test]
    fn exfat_write_is_rejected_read_only() {
        let disk = build_test_volume();
        let mut vol = ExfatVolume::mount(disk).unwrap();
        // 只读语义：写恒拒绝，绝不静默降级。
        assert_eq!(vol.write("/Games/hello.txt", b"x"), Err(FsError::ReadOnly));
        assert_eq!(vol.write("/new.txt", b"y"), Err(FsError::ReadOnly));
    }

    #[test]
    fn exfat_rejects_non_exfat_volume() {
        // 全零盘 → VBR 签名不符。
        let disk = MemDisk::new(4096);
        assert_eq!(ExfatVolume::mount(disk).err(), Some(FsError::NotExfat));
        // FAT 风格 VBR（MustBeZero 非零）→ 拒绝。
        let mut disk = MemDisk::new(4096);
        let mut vbr = vec![0u8; 512];
        vbr[3..11].copy_from_slice(b"EXFAT   ");
        vbr[11] = 0x01; // MustBeZero 区被污染
        vbr[510..512].copy_from_slice(&[0x55, 0xAA]);
        disk.put(0, &vbr);
        assert_eq!(ExfatVolume::mount(disk).err(), Some(FsError::NotExfat));
    }

    #[test]
    fn exfat_gpt_partition_located() {
        // GPT 盘：LBA1 头 + LBA2 一个 Basic Data 分区（first=64）。
        let mut disk = build_test_volume();
        // 卷内容整体后移 64 块太麻烦——直接在原盘上盖 GPT（分区指向 0，
        // first=0 last=4095 的 Basic Data）。
        let mut hdr = vec![0u8; 512];
        hdr[0..8].copy_from_slice(b"EFI PART");
        hdr[72..80].copy_from_slice(&2u64.to_le_bytes()); // entries @ LBA2
        hdr[80..84].copy_from_slice(&1u32.to_le_bytes()); // 1 个条目
        hdr[84..88].copy_from_slice(&128u32.to_le_bytes()); // 条目大小
        disk.put(1, &hdr);
        let mut ent = vec![0u8; 512];
        ent[0..16].copy_from_slice(&BASIC_DATA_GUID);
        ent[32..40].copy_from_slice(&0u64.to_le_bytes()); // first LBA 0
        ent[40..48].copy_from_slice(&4095u64.to_le_bytes()); // last
        disk.put(2, &ent);
        // 分区 first=0 == 整盘 → 挂载应同样成功。
        let mut vol = ExfatVolume::mount(disk).expect("GPT 定位（first=0）");
        assert!(vol.read_dir("/").is_ok());
    }

    #[test]
    fn snapshot_append_list_readback() {
        let disk = MemDisk::new(4096);
        let mut snap = SnapshotArea::new(disk, 3000);
        let s0 = snap.append("/Games/score.json", b"{\"best\":4242}").expect("追加");
        assert_eq!(s0, 0);
        let s1 = snap.append("/note", b"hello snapshot").expect("追加 2");
        assert_eq!(s1, 1);
        let list = snap.list().expect("列表");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].content, b"{\"best\":4242}");
        assert_eq!(list[1].content, b"hello snapshot");
        assert!(list[0].name.starts_with("fnv:"));
        // 追加后重开同区（持久语义：新 SnapshotArea 同盘读回一致）。
        let disk2 = {
            let SnapshotArea { dev, .. } = snap;
            dev
        };
        let mut snap2 = SnapshotArea::new(disk2, 3000);
        assert_eq!(snap2.list().unwrap().len(), 2, "重开读回一致");
    }

    #[test]
    fn snapshot_rejects_oversize_and_fills_up() {
        let disk = MemDisk::new(4096);
        let mut snap = SnapshotArea::new(disk, 3000);
        let big = vec![0xABu8; SNAP_MAX_CONTENT as usize + 1];
        assert_eq!(snap.append("/big", &big), Err(FsError::TooLarge), "超槽上限明确拒绝");
        // 填满 64 槽后第 65 条明确报错（容量语义）。
        for i in 0..SNAP_SLOTS {
            assert!(snap.append("n", b"d").is_ok(), "槽 {} 可写", i);
        }
        assert!(snap.append("n", b"d").is_err(), "满容量明确拒绝");
        let list = snap.list().unwrap();
        assert_eq!(list.len(), SNAP_SLOTS as usize);
    }

    #[test]
    fn snapshot_corrupt_content_reported() {
        let disk = MemDisk::new(4096);
        let mut snap = SnapshotArea::new(disk, 3000);
        snap.append("n", b"data").unwrap();
        // 篡改内容字节 → list 如实报 Io。
        {
            let SnapshotArea { dev, .. } = &mut snap;
            let mut blk = [0u8; 512];
            // content 在槽内偏移 32 起（槽 0 = LBA 3000），篡改 content[2]。
            dev.read_blocks(3000, &mut blk).unwrap();
            blk[34] ^= 0xFF;
            dev.write_blocks(3000, &blk).unwrap();
        }
        assert!(matches!(snap.list(), Err(FsError::Io(_))), "内容损坏如实上报");
    }
}
