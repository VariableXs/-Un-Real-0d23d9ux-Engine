//! S4.2-B · exFAT 受限直写层——AI-5 内核基建长线（2026-09-22）。
//!
//! **范围如实声明（与 [`super::exfat_ro`] 的只读语义互补）**：本模块提供
//! 两条**最小受控**写路径，其余一切写请求不存在：
//! 1. [`ExfatRw::rewrite_same_size`]——既有文件**同尺寸**就地改写：只写
//!    文件自身数据簇的起始扇区（内容按 512B 扇区边界零填充后落盘），
//!    绝不分配/释放簇、绝不改 FAT/位图/目录项/尺寸字段。窗口硬约束：
//!    文件 ≤ [`MAX_WINDOW_BYTES`]（JSON 契约文件远小于此）。
//! 2. [`ExfatRw::create_file_root`]——根目录新建小文件（≤15 字符 ASCII
//!    名、数据 ≤ 4KiB）：落盘顺序 = 数据扇区 → FAT 项 → 位图 → 目录项
//!    （目录项是提交点：断电在目录项前 = 孤儿簇，chkdsk 可回收，卷
//!    保持一致；全部写都是 ≤2 扇区的读-改-写）。
//!
//! **扇区粒度硬纪律**：真实 SHARED 卷（560GiB exFAT）簇可达 128KiB，
//! 内核堆单块 ≤4KiB——本层**从不物化整簇**：目录扫描按 512B 扇区流式
//! 进行（条目 32B 对齐，512%32==0，条目集跨扇区由 ≤576B 携带缓冲拼接）；
//! 数据/位图/FAT/目录项全部是 1–2 扇区的读-改-写。
//!
//! **不做**：删除/改尺寸/子目录写/目录链扩展/多簇文件创建/时间戳维护
//! （固定合法时间戳）/非 ASCII 名。写路径闸门：上层（挂载侧 vfsguard
//! 白名单 + 快照）负责策略，本层只提供原语并做几何/越界硬校验。
//! 布局常量与 [`super::exfat_ro`] 同源（exFAT spec 1.00 / GRUB 交叉）；
//! 挂载定位复用 `exfat_ro::gpt_first_basic_data`。

use super::exfat_ro::{gpt_first_basic_data, Bpb, FsError};
use crate::drivers::blk::{BlockDevice, BlockError};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// 单次数据窗口上限（= MSC 单命令 4KiB 缓冲；契约文件均远小于此）。
pub const MAX_WINDOW_BYTES: u64 = 4096;
/// 携带缓冲上限（最大条目集：FILE + 17 secondary = 18×32B）。
const SET_MAX: usize = 18 * 32;

// -- 目录项类型（写路径精确类型；与 exfat_ro 读侧常量同值）------------------
const ET_END: u8 = 0x00;
const ET_BITMAP: u8 = 0x81;
const ET_FILE: u8 = 0x85;
const ET_STREAM: u8 = 0xC0;
const ET_NAME: u8 = 0xC1;

const FLAG_IN_USE: u8 = 0x80;
const FLAG_SECONDARY_VALID: u8 = 0x40;
/// Stream GeneralSecondaryFlags bit1 = NoFatChain。
const STREAM_NO_FAT_CHAIN: u8 = 0x02;
/// 分配位图 FAT 链终止。
const EOC: u32 = 0xFFFF_FFFF;
/// FAT 有效簇范围下界。
const FAT_MIN: u32 = 2;

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

/// 文件定位产物。
struct FileLoc {
    /// 文件首簇（数据起始）。
    first_cluster: u32,
    size: u64,
    /// STREAM 条目所在（根目录链簇序号, 簇内偏移）——增长改写要更新
    /// Size/ValidDataLength 字段（单扇区 RMW）。
    stream_pos: Option<(usize, usize)>,
}

/// exFAT 受限直写卷。
pub struct ExfatRw<B: BlockDevice> {
    dev: B,
    part_base: u64,
    bpb: Bpb,
}

/// 扫描产物：一个目录条目集的语义视图。
struct EntryView {
    is_file_set: bool,
    /// FILE 条目属性（is_dir）。
    is_dir: bool,
    /// 名字（ASCII 语义，与读侧一致）。
    name: String,
    first_cluster: u32,
    size: u64,
    contiguous: bool,
    /// 条目集字节（拼接完整；长度 = (1+secondary)×32）。
    raw: Vec<u8>,
}

impl<B: BlockDevice> ExfatRw<B> {
    /// 挂载：GPT 定位（无 GPT 整盘）→ VBR/BPB 校验（与 exfat_ro 同口径）。
    pub fn mount(mut dev: B) -> Result<Self, FsError> {
        let part_base = match gpt_first_basic_data(&mut dev)? {
            Some(lba) => lba,
            None => 0,
        };
        let mut vbr = [0u8; 512];
        dev.read_blocks(part_base, &mut vbr)?;
        let bpb = Bpb::parse(&vbr).ok_or(FsError::NotExfat)?;
        let cap = dev.capacity_blocks();
        if part_base + bpb.volume_length_sectors > cap {
            return Err(FsError::NotExfat);
        }
        Ok(ExfatRw { dev, part_base, bpb })
    }

    fn sector_bytes(&self) -> usize {
        self.bpb.bytes_per_sector() as usize
    }

    fn cluster_lba(&self, cluster: u32) -> u64 {
        self.part_base + self.bpb.cluster_lba(cluster)
    }

    /// 沿 FAT 链收集簇号（扇区粒度；与 exfat_ro::fat_chain 同语义）。
    fn fat_chain(&mut self, first: u32, max: usize) -> Result<Vec<u32>, FsError> {
        let sb = self.sector_bytes();
        let mut chain = Vec::new();
        let mut c = first;
        let per_block = sb / 4;
        let mut buf = vec![0u8; sb];
        let mut cur_block = u64::MAX;
        while (FAT_MIN..0xFFFFFFF8).contains(&c) {
            chain.push(c);
            if chain.len() > max {
                return Err(FsError::Io(BlockError::Io));
            }
            let idx = (c - FAT_MIN) as usize;
            let block = (idx / per_block) as u64;
            if block != cur_block {
                let lba = self.part_base + self.bpb.fat_offset as u64 + block;
                self.dev.read_blocks(lba, &mut buf)?;
                cur_block = block;
            }
            let off = (idx % per_block) * 4;
            c = u32le(&buf[off..off + 4]);
        }
        if !(0xFFFFFFF8..=0xFFFFFFFF).contains(&c) && c != 0 {
            return Err(FsError::Io(BlockError::Io));
        }
        Ok(chain)
    }

    /// 根目录簇链。
    fn root_chain(&mut self) -> Result<Vec<u32>, FsError> {
        self.fat_chain(self.bpb.root_cluster, self.bpb.cluster_count as usize + 2)
    }

    /// **流式目录扫描**：按扇区读根目录链，32B 条目对齐拼接条目集
    /// （携带缓冲 ≤ SET_MAX），每完整条目集回调一次（含链内位置 (簇序号, 簇内偏移)）。
    fn scan_root(&mut self, mut on_set: impl FnMut(&EntryView, (usize, usize))) -> Result<(), FsError> {
        let sb = self.sector_bytes();
        let chain = self.root_chain()?;
        let cb = self.bpb.cluster_bytes() as usize;
        let sectors_per_cluster = cb / sb;
        let mut carry: Vec<u8> = Vec::new();
        let mut carry_pos: (usize, usize) = (0, 0);
        let mut sbuf = vec![0u8; sb];
        for (ci, &cluster) in chain.iter().enumerate() {
            let base_lba = self.cluster_lba(cluster);
            for s in 0..sectors_per_cluster as u64 {
                self.dev.read_blocks(base_lba + s, &mut sbuf)?;
                for e in 0..sb / 32 {
                    let ent = &sbuf[e * 32..e * 32 + 32];
                    let t = ent[0];
                    let pos = (ci, (s as usize) * sb + e * 32);
                    if carry.is_empty() {
                        if t == ET_END {
                            // End-of-Directory：本簇余下不再有有效条目。
                            break;
                        }
                        if t & FLAG_IN_USE == 0 {
                            continue; // 已删除槽位。
                        }
                        if t != ET_FILE {
                            // 位图/UPC/卷标等非 File 集合：单条目即完整。
                            let view = EntryView {
                                is_file_set: false,
                                is_dir: false,
                                name: String::new(),
                                first_cluster: u32le(&ent[20..24]),
                                size: u64le(&ent[24..32]),
                                contiguous: false,
                                raw: ent.to_vec(),
                            };
                            on_set(&view, pos);
                            continue;
                        }
                        carry_pos = pos;
                    }
                    carry.extend_from_slice(ent);
                    if carry.len() >= 32 {
                        let nsec = carry[1] as usize;
                        let total = (1 + nsec) * 32;
                        if nsec == 0 || nsec > 17 || carry.len() > SET_MAX {
                            // 非法 SecondaryCount：放弃本集（如实容错）。
                            carry.clear();
                            continue;
                        }
                        if carry.len() == total {
                            // 拼接完成：抽语义。
                            let mut name = String::new();
                            let (mut first_cluster, mut size, mut contiguous) = (0u32, 0u64, false);
                            let mut have_stream = false;
                            let mut is_dir = false;
                            for k in 1..=nsec {
                                let se = &carry[k * 32..k * 32 + 32];
                                let st = se[0];
                                if st & FLAG_IN_USE == 0 || st & FLAG_SECONDARY_VALID == 0 {
                                    continue;
                                }
                                match st {
                                    ET_STREAM => {
                                        first_cluster = u32le(&se[20..24]);
                                        size = u64le(&se[24..32]);
                                        contiguous = se[1] & STREAM_NO_FAT_CHAIN != 0;
                                        have_stream = true;
                                    }
                                    ET_NAME => {
                                        for j in 0..15 {
                                            let u = u16le(&se[2 + j * 2..4 + j * 2]);
                                            if u == 0 {
                                                break;
                                            }
                                            if (0x20..0x7F).contains(&u) {
                                                name.push(u as u8 as char);
                                            } else {
                                                name.push('\u{FFFD}');
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            if carry.len() >= 6 {
                                is_dir = u16le(&carry[4..6]) & 0x10 != 0;
                            }
                            if have_stream && !name.is_empty() {
                                let view = EntryView {
                                    is_file_set: true,
                                    is_dir,
                                    name,
                                    first_cluster,
                                    size,
                                    contiguous,
                                    raw: carry.clone(),
                                };
                                on_set(&view, carry_pos);
                            }
                            carry.clear();
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// 定位根目录直接子文件（流式；返回首簇+尺寸+STREAM 条目位置）。
    fn locate_root_file(&mut self, name: &str) -> Result<FileLoc, FsError> {
        let mut hit = None;
        self.scan_root(&mut |v: &EntryView, pos: (usize, usize)| {
            if v.is_file_set && !v.is_dir && v.name == name && hit.is_none() {
                hit = Some((v.first_cluster, v.size, v.contiguous, pos));
            }
        })?;
        let Some((first, size, contiguous, pos)) = hit else {
            return Err(FsError::NotFound);
        };
        // 链校验：非连续链沿 FAT 走一环确认起点合法（越界/空簇拒绝）。
        if !contiguous && size > 0 {
            let chain = self.fat_chain(first, 2)?;
            if chain.is_empty() {
                return Err(FsError::Io(BlockError::Io));
            }
        }
        if first < FAT_MIN {
            return Err(FsError::Io(BlockError::Io));
        }
        Ok(FileLoc { first_cluster: first, size, stream_pos: Some(pos) })
    }

    /// **同尺寸就地改写**（根目录直接子文件，窗口 ≤ [`MAX_WINDOW_BYTES`]）。
    ///
    /// - `data.len() ≤ size`：内容按扇区边界零填充后写入文件数据起始
    ///   扇区（文件自身簇内，尺寸字段零改动；读侧以 size 截断）。
    /// - `data.len() > size`：显式拒绝（用 [`Self::grow_rewrite_root_file`]）。
    /// - `size > MAX_WINDOW_BYTES`：拒绝（多簇改写断电撕裂窗口不可接受）。
    pub fn rewrite_same_size(&mut self, name: &str, data: &[u8]) -> Result<(), FsError> {
        let loc = self.locate_root_file(name)?;
        if loc.size > MAX_WINDOW_BYTES {
            return Err(FsError::TooLarge);
        }
        if data.len() as u64 > loc.size {
            return Err(FsError::TooLarge); // 增大 = 改尺寸，本层不做。
        }
        if loc.first_cluster < FAT_MIN {
            return Err(FsError::Io(BlockError::Io));
        }
        // 内容补零到扇区边界（文件自身簇内；超出 size 的部分读侧不可见）。
        let sb = self.sector_bytes();
        let mut sect = vec![0u8; sb];
        if data.len() > sb {
            // >1 扇区窗口：逐扇区写入（仍是文件自身簇内的既有扇区）。
            let n = data.len().div_ceil(sb);
            for k in 0..n {
                let start = k * sb;
                let end = core::cmp::min(start + sb, data.len());
                sect[..end - start].copy_from_slice(&data[start..end]);
                self.dev.write_blocks(self.cluster_lba(loc.first_cluster) + k as u64, &sect)?;
                sect.fill(0);
            }
        } else {
            sect[..data.len()].copy_from_slice(data);
            self.dev.write_blocks(self.cluster_lba(loc.first_cluster), &sect)?;
        }
        self.dev.flush()?;
        Ok(())
    }

    /// **同簇增长改写**（根目录直接子文件）：数据可超过原尺寸（≤
    /// [`MAX_WINDOW_BYTES`]，且必须仍落在首簇内），写数据后更新 STREAM
    /// 条目的 Size/ValidDataLength（父目录单扇区 RMW，**提交点最后写**）。
    /// 断电窗口：数据已写而尺寸未更新 = 读侧看到旧尺寸的新前缀
    /// （last_boot 场景=合法 JSON 前缀，解析回退契约兜底）。
    pub fn grow_rewrite_root_file(&mut self, name: &str, data: &[u8]) -> Result<(), FsError> {
        let loc = self.locate_root_file(name)?;
        if data.len() as u64 > MAX_WINDOW_BYTES {
            return Err(FsError::TooLarge);
        }
        if data.len() as u64 <= loc.size {
            return self.rewrite_same_size(name, data); // 缩短走同尺寸路径
        }
        if loc.first_cluster < FAT_MIN {
            return Err(FsError::Io(BlockError::Io));
        }
        let Some((ci, off)) = loc.stream_pos else {
            return Err(FsError::Io(BlockError::Io));
        };
        // STREAM 条目必须完整落在同一扇区（32B 对齐保证）。
        let sb = self.sector_bytes();
        let chain = self.root_chain()?;
        let dir_lba = self.cluster_lba(chain[ci]) + (off / sb) as u64;
        // ① 数据扇区（尺寸提交前：读侧仍看旧尺寸）。
        let mut sect = vec![0u8; sb];
        let n = data.len().div_ceil(sb);
        for k in 0..n {
            let start = k * sb;
            let end = core::cmp::min(start + sb, data.len());
            sect[..end - start].copy_from_slice(&data[start..end]);
            self.dev.write_blocks(self.cluster_lba(loc.first_cluster) + k as u64, &sect)?;
            sect.fill(0);
        }
        // ② 目录项尺寸字段（提交点）：STREAM+8(ValidDataLength)/+24(Size)。
        let in_off = off % sb;
        self.dev.read_blocks(dir_lba, &mut sect)?;
        let stream = in_off + 32; // STREAM = FILE 之后第一条 secondary
        if stream + 32 > sb {
            return Err(FsError::Io(BlockError::Io)); // 跨扇区条目：拒绝
        }
        sect[stream + 8..stream + 16].copy_from_slice(&(data.len() as u64).to_le_bytes());
        sect[stream + 24..stream + 32].copy_from_slice(&(data.len() as u64).to_le_bytes());
        self.dev.write_blocks(dir_lba, &sect)?;
        self.dev.flush()?;
        Ok(())
    }

    /// **根目录新建小文件**（名字 1..=15 ASCII，数据 ≤ [`MAX_WINDOW_BYTES`]）。
    /// 落盘顺序：数据扇区 → FAT 项（EOC）→ 位图置位 → 目录项（提交点）。
    pub fn create_file_root(&mut self, name: &str, data: &[u8]) -> Result<(), FsError> {
        if name.is_empty() || name.len() > 15 || !name.bytes().all(|b| (0x21..0x7F).contains(&b)) {
            return Err(FsError::BadPath);
        }
        if data.is_empty() || data.len() > MAX_WINDOW_BYTES as usize {
            return Err(FsError::TooLarge);
        }
        // 重名拒绝（只增不覆：同名文件存在即拒绝）。
        if self.locate_root_file(name).is_ok() {
            return Err(FsError::Io(BlockError::Io));
        }
        // ① 扫描根目录：位图条目簇号 + 空位（连续 3 个空闲/END 条目）。
        let need = 3 * 32; // FILE + STREAM + NAME
        let mut bitmap_cluster = None;
        let mut free_slot: Option<(u32, usize)> = None; // (簇号, 簇内偏移)
        let mut free_cnt = 0usize;
        let mut free_ci = 0usize;
        let mut free_start = 0usize;
        let mut free_last: i64 = -64;
        {
            let sb = self.sector_bytes();
            let cb = self.bpb.cluster_bytes() as usize;
            let chain = self.root_chain()?;
            let spc = cb / sb;
            let mut sbuf = vec![0u8; sb];
            'outer: for (ci, &cluster) in chain.iter().enumerate() {
                let base_lba = self.cluster_lba(cluster);
                for s in 0..spc as u64 {
                    self.dev.read_blocks(base_lba + s, &mut sbuf)?;
                    for e in 0..sb / 32 {
                        let ent = &sbuf[e * 32..e * 32 + 32];
                        let off = (s as usize) * sb + e * 32;
                        let t = ent[0];
                        if t == ET_BITMAP && ent[0] & FLAG_IN_USE != 0 {
                            bitmap_cluster = Some(u32le(&ent[20..24]));
                        }
                        if t & FLAG_IN_USE == 0 {
                            // 空闲/END 槽位：续跑或开新跑。
                            if free_cnt == 0 || free_ci != ci || free_last + 32 != off as i64 {
                                free_cnt = 1;
                                free_ci = ci;
                                free_start = off;
                            } else {
                                free_cnt += 1;
                            }
                            free_last = off as i64;
                            if free_cnt * 32 >= need && free_slot.is_none() {
                                free_slot = Some((chain[free_ci], free_start));
                                break 'outer;
                            }
                        } else {
                            free_cnt = 0;
                        }
                    }
                }
            }
        }
        let bitmap_cluster = bitmap_cluster.ok_or(FsError::Io(BlockError::Io))?;
        let (slot_cluster, slot_off) = free_slot.ok_or(FsError::Io(BlockError::Io))?;
        // ② 分配空闲簇（位图扇区级扫描到首个 0 位）。
        let sb = self.sector_bytes();
        let mut sect = vec![0u8; sb];
        let bitmap_bits_per_sector = sb * 8;
        let mut new_cluster = None;
        let mut new_bit_sector = 0u64;
        let mut new_bit_off = 0usize;
        {
            let total_bits = self.bpb.cluster_count as usize;
            let mut done = 0usize;
            while done < total_bits {
                let sector_in_bitmap = (done / bitmap_bits_per_sector) as u64;
                self.dev.read_blocks(self.cluster_lba(bitmap_cluster) + sector_in_bitmap, &mut sect)?;
                for i in done..core::cmp::min(done + bitmap_bits_per_sector, total_bits) {
                    let bit_off = i - (sector_in_bitmap as usize) * bitmap_bits_per_sector;
                    if sect[bit_off / 8] & (1 << (bit_off % 8)) == 0 {
                        new_cluster = Some(i as u32 + FAT_MIN);
                        new_bit_sector = sector_in_bitmap;
                        new_bit_off = bit_off;
                        break;
                    }
                }
                if new_cluster.is_some() {
                    break;
                }
                done += bitmap_bits_per_sector;
            }
        }
        let cluster = new_cluster.ok_or(FsError::Io(BlockError::Io))?;
        // ③ 数据扇区（提交点前：孤簇无害）。零填充到扇区边界。
        {
            let n = data.len().div_ceil(sb);
            let mut buf = vec![0u8; sb];
            for k in 0..n {
                let start = k * sb;
                let end = core::cmp::min(start + sb, data.len());
                buf[..end - start].copy_from_slice(&data[start..end]);
                self.dev.write_blocks(self.cluster_lba(cluster) + k as u64, &buf)?;
                buf.fill(0);
            }
        }
        // ④ FAT 项 = EOC（扇区读-改-写）。
        {
            let idx = (cluster - FAT_MIN) as usize;
            let per_block = sb / 4;
            let lba = self.part_base + self.bpb.fat_offset as u64 + (idx / per_block) as u64;
            let off = (idx % per_block) * 4;
            self.dev.read_blocks(lba, &mut sect)?;
            sect[off..off + 4].copy_from_slice(&EOC.to_le_bytes());
            self.dev.write_blocks(lba, &sect)?;
        }
        // ⑤ 位图置位（扇区读-改-写）。
        {
            let lba = self.cluster_lba(bitmap_cluster) + new_bit_sector;
            self.dev.read_blocks(lba, &mut sect)?;
            sect[new_bit_off / 8] |= 1 << (new_bit_off % 8);
            self.dev.write_blocks(lba, &sect)?;
        }
        // ⑥ 目录项（提交点）：跨扇区读-改-写（96B ≤ 2 扇区窗口）。
        {
            let entries = Self::make_entry_set(name, cluster, data.len() as u64);
            let dir_lba = self.cluster_lba(slot_cluster) + (slot_off / sb) as u64;
            let in_off = slot_off % sb;
            let sectors = (in_off + need).div_ceil(sb);
            let mut window = vec![0u8; sectors * sb];
            self.dev.read_blocks(dir_lba, &mut window)?;
            window[in_off..in_off + need].copy_from_slice(&entries);
            self.dev.write_blocks(dir_lba, &window)?;
        }
        self.dev.flush()?;
        Ok(())
    }

    /// 构造 FILE+STREAM+NAME 三条目（含 SetChecksum；固定合法时间戳
    /// 2026-09-22 00:00；STREAM 声明 NoFatChain——单簇连续语义）。
    fn make_entry_set(name: &str, first_cluster: u32, size: u64) -> Vec<u8> {
        const DATE: u16 = ((2026 - 1980) << 9) | (9 << 5) | 22;
        const TIME: u16 = 0;
        let mut file = [0u8; 32];
        file[0] = ET_FILE;
        file[1] = 2; // SecondaryCount = STREAM + NAME
        file[4..6].copy_from_slice(&0x20u16.to_le_bytes()); // Attr = Archive
        file[8..10].copy_from_slice(&TIME.to_le_bytes());
        file[10..12].copy_from_slice(&DATE.to_le_bytes());
        file[12..14].copy_from_slice(&TIME.to_le_bytes());
        file[14..16].copy_from_slice(&DATE.to_le_bytes());
        let mut stream = [0u8; 32];
        stream[0] = ET_STREAM;
        stream[1] = STREAM_NO_FAT_CHAIN;
        stream[3] = name.len() as u8;
        stream[8..16].copy_from_slice(&size.to_le_bytes()); // ValidDataLength
        stream[20..24].copy_from_slice(&first_cluster.to_le_bytes());
        stream[24..32].copy_from_slice(&size.to_le_bytes()); // Size
        let mut name_e = [0u8; 32];
        name_e[0] = ET_NAME;
        for (k, b) in name.bytes().enumerate() {
            name_e[2 + k * 2..4 + k * 2].copy_from_slice(&(b as u16).to_le_bytes());
        }
        // SetChecksum（exFAT spec §7.9.1）：rotate-left 累加，FILE 条目
        // 的 2..4 校验和域本身不计。
        let mut csum: u16 = 0;
        for (ei, e) in [&file, &stream, &name_e].iter().enumerate() {
            for (bi, &b) in e.iter().enumerate() {
                if ei == 0 && (2..4).contains(&bi) {
                    continue;
                }
                csum = csum.rotate_left(1).wrapping_add(b as u16);
            }
        }
        file[2..4].copy_from_slice(&csum.to_le_bytes());
        let mut out = Vec::with_capacity(96);
        out.extend_from_slice(&file);
        out.extend_from_slice(&stream);
        out.extend_from_slice(&name_e);
        out
    }

    // ---- 读路径（回环验证用；扇区粒度最小集）-------------------------------

    /// 读根目录直接子文件（窗口 ≤ [`MAX_WINDOW_BYTES`]，扇区粒度）。
    pub fn read_root_file(&mut self, name: &str) -> Result<Vec<u8>, FsError> {
        let loc = self.locate_root_file(name)?;
        if loc.size > MAX_WINDOW_BYTES {
            return Err(FsError::TooLarge);
        }
        let sb = self.sector_bytes();
        let n = loc.size.div_ceil(sb as u64) as u64;
        let mut out = vec![0u8; (n as usize) * sb];
        self.dev
            .read_blocks(self.cluster_lba(loc.first_cluster), &mut out)?;
        out.truncate(loc.size as usize);
        Ok(out)
    }

    /// 列根目录（名字/尺寸/是否目录；流式）。
    pub fn list_root(&mut self) -> Result<Vec<(String, u64, bool)>, FsError> {
        let mut out = Vec::new();
        self.scan_root(&mut |v: &EntryView, _pos: (usize, usize)| {
            if v.is_file_set {
                out.push((v.name.clone(), v.size, v.is_dir));
            }
        })?;
        Ok(out)
    }
}

// -- 目标态：last_boot 写回总入口（S4.2 方案2 根解）--------------------------

/// 把 SHARED 卷 `boot-select.json` 的 `last_boot` 域改写为 `value`
/// （经任意块设备通道）。读现值 → 文本级拼接 → 受限改写；任一步失败
/// = false（调用方尽力而为语义，绝不阻塞引导）。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn write_last_boot_via(mut dev: &mut dyn BlockDevice, value: &str) -> bool {
    use alloc::string::ToString;
    let mut vol = match ExfatRw::mount(&mut dev) {
        Ok(v) => v,
        Err(e) => {
            crate::kwarn!("lastboot: shared mount failed {:?}", e);
            return false;
        }
    };
    let cur = match vol.read_root_file("boot-select.json") {
        Ok(b) => b,
        Err(e) => {
            crate::kwarn!("lastboot: read boot-select.json failed {:?}", e);
            return false;
        }
    };
    let Some(newjson) = crate::bootcfg::splice_last_boot(&cur, value) else {
        crate::kinfo!("lastboot: key absent (windows side will create) - skip");
        return false;
    };
    // 等长/缩短 → 零元数据改写；增长 → 同簇增长改写（尺寸字段提交点）。
    let r = if newjson.len() <= cur.len() {
        vol.rewrite_same_size("boot-select.json", &newjson)
    } else {
        vol.grow_rewrite_root_file("boot-select.json", &newjson)
    };
    match r {
        Ok(()) => {
            crate::kinfo!("lastboot: last_boot={} written ({} bytes)", value, newjson.len());
            true
        }
        Err(e) => {
            crate::kwarn!("lastboot: rewrite failed {:?}", e);
            false
        }
    }
}

/// last_boot 写回总入口：MSC（真机 U 盘直写）优先，QEMU 第二 NVMe
/// （usrshell 全局挂载的同一块设备）兜底。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn record_last_boot(value: &str) -> bool {
    let msc = crate::drivers::xhci::target::msc_block_device();
    crate::kinfo!("lastboot: msc channel present={}", msc.is_some());
    if let Some(mut blk) = msc {
        if write_last_boot_via(&mut blk, value) {
            return true;
        }
    }
    let r = crate::proc::usrshell::mount::with_shared_dev(|dev| write_last_boot_via(dev, value));
    crate::kinfo!("lastboot: nvme channel r={}", r);
    r
}

// -- 宿主测试 ---------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::blk::BlockDevice;
    use std::collections::BTreeMap;

    /// 内存块设备。
    #[derive(Clone)]
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
        fn snapshot(&self) -> MemDisk {
            self.clone()
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

    /// 断电注入盘：全局扇区写预算，耗尽即「掉电」（已写扇区保留）。
    #[derive(Clone)]
    struct FaultDisk {
        inner: MemDisk,
        budget: Option<u64>,
    }

    impl BlockDevice for FaultDisk {
        fn block_size(&self) -> u32 {
            512
        }
        fn capacity_blocks(&self) -> u64 {
            self.inner.capacity_blocks()
        }
        fn read_blocks(&mut self, lba: u64, dst: &mut [u8]) -> Result<(), BlockError> {
            self.inner.read_blocks(lba, dst)
        }
        fn write_blocks(&mut self, lba: u64, src: &[u8]) -> Result<(), BlockError> {
            let n = src.len() / 512;
            for i in 0..n as u64 {
                match self.budget.as_mut() {
                    Some(0) => return Err(BlockError::Io), // 掉电
                    Some(b) => *b -= 1,
                    None => {}
                }
                let mut blk = [0u8; 512];
                blk.copy_from_slice(&src[i as usize * 512..(i + 1) as usize * 512]);
                self.inner.blocks.insert(lba + i, blk);
            }
            Ok(())
        }
        fn flush(&mut self) -> Result<(), BlockError> {
            Ok(())
        }
    }

    // -- 独立 exFAT 构造器（含位图条目——写层测试契约）-----------------------

    const HEAP_START: u32 = 512;
    const FAT_OFFSET: u32 = 128;
    const CLUSTER_COUNT: u32 = 448;
    const CLUSTER_BYTES: usize = 4096;

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
            for c in 2u32..5 {
                b.fat[(c - 2) as usize] = 0xFFFF_FFFF; // bitmap/upcase/root
            }
            b
        }

        fn put_cluster(&mut self, c: u32, data: &[u8]) {
            let lba: u64 = HEAP_START as u64 + (c as u64 - 2) * (CLUSTER_BYTES as u64 / 512);
            let off = (lba * 512) as usize;
            self.img[off..off + data.len()].copy_from_slice(data);
        }

        fn alloc_chain(&mut self, n: usize) -> u32 {
            let first = self.next_alloc;
            for k in 0..n as u32 {
                let c = first + k;
                self.fat[(c - 2) as usize] = if k == n as u32 - 1 { 0xFFFF_FFFF } else { c + 1 };
            }
            self.next_alloc += n as u32;
            first
        }

        fn entry_set_checksum(entries: &[&[u8]]) -> u16 {
            let mut csum: u16 = 0;
            for (ei, e) in entries.iter().enumerate() {
                for (i, &b) in e.iter().enumerate() {
                    if ei == 0 && (2..4).contains(&i) {
                        continue;
                    }
                    csum = csum.rotate_left(1).wrapping_add(b as u16);
                }
            }
            csum
        }

        fn make_entry(name: &str, first: u32, size: u64) -> Vec<u8> {
            let units: Vec<u16> = name.encode_utf16().collect();
            let nname = units.len().div_ceil(15).max(1);
            let mut file = vec![0u8; 32];
            file[0] = 0x85;
            file[1] = (1 + nname) as u8;
            file[4..6].copy_from_slice(&0x20u16.to_le_bytes());
            let mut stream = vec![0u8; 32];
            stream[0] = 0xC0;
            stream[3] = units.len() as u8;
            stream[20..24].copy_from_slice(&first.to_le_bytes());
            stream[24..32].copy_from_slice(&size.to_le_bytes());
            let mut names: Vec<Vec<u8>> = Vec::new();
            let mut padded = units.clone();
            padded.resize(nname * 15, 0);
            for k in 0..nname {
                let mut e = vec![0u8; 32];
                e[0] = 0xC1;
                for j in 0..15 {
                    e[2 + j * 2..4 + j * 2].copy_from_slice(&padded[k * 15 + j].to_le_bytes());
                }
                names.push(e);
            }
            let all: Vec<&[u8]> = std::iter::once(&file[..])
                .chain(std::iter::once(&stream[..]))
                .chain(names.iter().map(|v| &v[..]))
                .collect();
            let csum = Self::entry_set_checksum(&all);
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

        fn finish(mut self, root_entries: &[u8]) -> MemDisk {
            let vbr = self.vbr();
            self.img[0..512].copy_from_slice(&vbr);
            self.img[512..1024].copy_from_slice(&vbr);
            let fat_off = (FAT_OFFSET * 512) as usize;
            for (k, v) in self.fat.iter().enumerate() {
                self.img[fat_off + k * 4..fat_off + k * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
            // bitmap（簇 2）：簇 2..next_alloc 已用。
            let mut bm = [0u8; CLUSTER_BYTES];
            for c in 2..self.next_alloc {
                bm[((c - 2) / 8) as usize] |= 1 << ((c - 2) % 8);
            }
            self.put_cluster(2, &bm);
            self.put_cluster(3, &[0u8; CLUSTER_BYTES]); // upcase 占位
            let mut root: Vec<u8> = Vec::new();
            let mut bitmap_e = vec![0u8; 32];
            bitmap_e[0] = 0x81;
            bitmap_e[20..24].copy_from_slice(&2u32.to_le_bytes());
            bitmap_e[24..32].copy_from_slice(&((CLUSTER_COUNT / 8) as u64).to_le_bytes());
            root.extend_from_slice(&bitmap_e);
            root.extend_from_slice(root_entries);
            root.extend_from_slice(&[0u8; 32]);
            root.resize(CLUSTER_BYTES, 0);
            self.put_cluster(4, &root);
            let mut disk = MemDisk::new(4096);
            for lba in 0..4096u64 {
                disk.put(lba, &self.img[(lba * 512) as usize..(lba * 512 + 512) as usize]);
            }
            disk
        }
    }

    /// 预置卷：/boot-select.json（100B）+ /big.json（2 簇 8KiB）。
    fn build_volume() -> MemDisk {
        let mut b = Builder::new();
        let mut boot = b"{\"handoff\":true,\"timeout\":5,\"last_boot\":\"windows\",\"pad\"".to_vec();
        boot.resize(100, b' ');
        let boot_c = b.alloc_chain(1);
        b.put_cluster(boot_c, &boot);
        let boot_e = Builder::make_entry("boot-select.json", boot_c, boot.len() as u64);
        let big: Vec<u8> = (0..2 * CLUSTER_BYTES).map(|i| (i * 7) as u8).collect();
        let big_c = b.alloc_chain(2);
        b.put_cluster(big_c, &big[..CLUSTER_BYTES]);
        b.put_cluster(big_c + 1, &big[CLUSTER_BYTES..]);
        let big_e = Builder::make_entry("big.json", big_c, big.len() as u64);
        let mut root = boot_e;
        root.extend(big_e);
        b.finish(&root)
    }

    // -- 用例 ---------------------------------------------------------------

    #[test]
    fn exfatrw_same_size_rewrite_roundtrip() {
        let disk = build_volume();
        let mut vol = ExfatRw::mount(disk).expect("挂载必须成功");
        let orig = vol.read_root_file("boot-select.json").expect("预置文件可读");
        assert_eq!(orig.len(), 100);
        let new_json = b"{\"handoff\":false,\"timeout\":3,\"last_boot\":\"variable\",\"p";
        vol.rewrite_same_size("boot-select.json", new_json).expect("等长改写必须成功");
        let back = vol.read_root_file("boot-select.json").expect("读回");
        assert_eq!(back.len(), 100, "尺寸字段零改动");
        assert_eq!(&back[..new_json.len()], &new_json[..]);
        assert!(back[new_json.len()..].iter().all(|&b| b == 0x20 || b == 0),
            "尺寸窗口外保持卷面原样");
    }

    #[test]
    fn exfatrw_rewrite_shorter_pads_to_old_size() {
        let disk = build_volume();
        let mut vol = ExfatRw::mount(disk).unwrap();
        let shorter = b"{\"last_boot\":\"variable\"}";
        vol.rewrite_same_size("boot-select.json", shorter).expect("缩短改写必须成功");
        let back = vol.read_root_file("boot-select.json").unwrap();
        assert_eq!(back.len(), 100, "尺寸不变（尺寸字段零改动）");
        assert_eq!(&back[..shorter.len()], &shorter[..]);
    }

    #[test]
    fn exfatrw_rewrite_rejects_growth_and_oversize_and_missing() {
        let disk = build_volume();
        let mut vol = ExfatRw::mount(disk).unwrap();
        // rewrite_same_size 拒绝增长（增长走 grow_rewrite_root_file）。
        let long = vec![0x41u8; 101];
        assert_eq!(vol.rewrite_same_size("boot-select.json", &long), Err(FsError::TooLarge));
        assert_eq!(vol.rewrite_same_size("big.json", &[0u8; 100]), Err(FsError::TooLarge));
        assert_eq!(vol.rewrite_same_size("nope.json", &[0u8; 10]), Err(FsError::NotFound));
    }

    #[test]
    fn exfatrw_create_file_visible_and_readable() {
        let disk = build_volume();
        let mut vol = ExfatRw::mount(disk).unwrap();
        vol.create_file_root("varix-loop.txt", b"loop-evidence-42").expect("新建必须成功");
        let items = vol.list_root().unwrap();
        assert!(items.iter().any(|(n, s, d)| n == "varix-loop.txt" && *s == 16 && !*d));
        let back = vol.read_root_file("varix-loop.txt").expect("新建文件可读");
        assert_eq!(back, b"loop-evidence-42");
        // 重复名拒绝。
        assert!(vol.create_file_root("varix-loop.txt", b"x").is_err());
        // 非法名拒绝。
        assert!(vol.create_file_root("", b"x").is_err());
        assert!(vol.create_file_root("too-long-name-exceeds-15", b"x").is_err());
        // 既有文件不受影响。
        assert_eq!(vol.read_root_file("boot-select.json").unwrap().len(), 100);
    }

    #[test]
    fn exfatrw_create_power_cut_never_torn_visible_file() {
        // 断电注入：create 写序列 = 数据 1 + FAT 1 + 位图 1 + 目录 1 = 4 扇区。
        // 预算 0..=4 逐一模拟：提交点（目录项）前掉电 = 文件不可见；全部
        // 落盘 = 内容完整。绝不出现「可见但撕裂数据」。
        let clean = build_volume();
        for budget in 0u64..=4u64 {
            let mut disk = clean.snapshot();
            let mut vol2 = match ExfatRw::mount(FaultDisk { inner: disk.clone(), budget: Some(budget) }) {
                Ok(v) => v,
                Err(e) => panic!("budget={budget}: 挂载失败 {e:?}"),
            };
            let _ = vol2.create_file_root("varix-loop.txt", b"loop-evidence-42");
            disk = match vol2 {
                ExfatRw { dev: FaultDisk { inner, .. }, .. } => inner,
            };
            let mut vol3 = ExfatRw::mount(disk).expect("掉电后卷必须仍可挂载");
            let items = vol3.list_root().expect("根目录必须可解析");
            match items.iter().find(|(n, ..)| n == "varix-loop.txt") {
                None => {}
                Some((_, s, d)) => {
                    assert_eq!((*s, *d), (16, false), "budget={budget}: 可见即必须完整");
                    assert_eq!(vol3.read_root_file("varix-loop.txt").unwrap(), b"loop-evidence-42");
                }
            }
        }
    }

    #[test]
    fn exfatrw_rewrite_power_cut_never_breaks_volume() {
        // 就地改写掉电：数据扇区部分写入 → 卷可挂载、目录项/位图不受影响。
        let clean = build_volume();
        for budget in 0u64..=1u64 {
            let mut disk = clean.snapshot();
            let mut vol2 = match ExfatRw::mount(FaultDisk { inner: disk.clone(), budget: Some(budget) }) {
                Ok(v) => v,
                Err(e) => panic!("budget={budget}: 挂载失败 {e:?}"),
            };
            let _ = vol2.rewrite_same_size("boot-select.json", b"{\"last_boot\":\"variable\"}");
            disk = match vol2 {
                ExfatRw { dev: FaultDisk { inner, .. }, .. } => inner,
            };
            let mut vol3 = ExfatRw::mount(disk).expect("掉电后卷必须仍可挂载");
            let items = vol3.list_root().unwrap();
            assert!(items.iter().any(|(n, s, _)| n == "boot-select.json" && *s == 100),
                "budget={budget}: 目录项必须完好");
        }
    }

    #[test]
    fn exfatrw_create_rejects_when_root_full() {
        // 根目录塞满 → 无 3 连空位 → 显式拒绝（目录扩展 v1 不做）。
        let mut b = Builder::new();
        let mut root = Vec::new();
        for i in 0..42usize {
            let c = b.alloc_chain(1);
            b.put_cluster(c, &[0u8; CLUSTER_BYTES]);
            root.extend(Builder::make_entry(&format!("f{:03}", i), c, 0));
        }
        let disk = b.finish(&root);
        let mut vol = ExfatRw::mount(disk).unwrap();
        assert!(matches!(vol.create_file_root("varix-loop.txt", b"x"), Err(FsError::Io(_))));
    }

    #[test]
    fn exfatrw_grow_rewrite_updates_size_field() {
        // windows(7)→variable(8) 的真实场景：JSON 增长 1 字节以上的改写。
        let disk = build_volume();
        let mut vol = ExfatRw::mount(disk).unwrap();
        let before = vol.read_root_file("boot-select.json").unwrap();
        assert_eq!(before.len(), 100);
        // 构造 134B 新内容（比原 100B 长）：合法 JSON（windows→variable 增长场景）。
        let mut new_json = b"{\"default_entry\":\"variable\",\"timeout_sec\":5,\"show_menu\":true,\"last_boot\":\"variable\",\"handoff\":true,\"note\":\"grow-case-verified\"}".to_vec();
        assert!(new_json.len() > 100);
        vol.grow_rewrite_root_file("boot-select.json", &new_json)
            .expect("同簇增长改写必须成功");
        let back = vol.read_root_file("boot-select.json").unwrap();
        assert_eq!(back.len(), new_json.len(), "尺寸字段必须已更新");
        assert_eq!(back, new_json);
        // 卷其余部分不受影响。
        let items = vol.list_root().unwrap();
        assert!(items.iter().any(|(n, _, _)| n == "big.json"));
    }

    #[test]
    fn exfatrw_streaming_scan_handles_multi_name_entries() {
        // 16 字符名（跨 2 个 NAME 条目）可被流式扫描正确拼接。
        let disk = build_volume();
        let mut vol = ExfatRw::mount(disk).unwrap();
        let items = vol.list_root().unwrap();
        assert!(items.iter().any(|(n, ..)| n == "boot-select.json"));
        assert!(items.iter().any(|(n, ..)| n == "big.json"));
    }
}
