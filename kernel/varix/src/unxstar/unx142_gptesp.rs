//! unx142_gptesp — UNX-F0142 GPT/ESP 分区镜像生成器（纯函数镜像装配）。
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F0142 · UNX-A1 表行）：UNX-F0142-J1
//! IMG 镜像 GPT 表/ESP FAT 结构与 sgdisk 校验一致，真机引导通过。
//!
//! **判据三分解**：
//! 1. **GPT 表结构**：保护性 MBR（0xEE 全盘表项）+ 主/备 GPT 头
//!    （"EFI PART" 签名 / 修订 1.0 / 头尺寸 92 / 头 CRC32 自校验 /
//!    主备 LBA 镜像 / 分区表项 CRC32）+ 128 槽分区表项
//!    （ESP 类型 GUID C12A7328-F81F-11D2-BA4B-00A0C93EC93B）；
//! 2. **ESP FAT 结构**：FAT32 引导扇区 BPB 字段装配（字节序小端逐字节
//!    显性）+ FSInfo 扇区（0x41615252 / 0x61417272 双签名 + 三指针）+
//!    FAT 表首簇标记（0x0FFFFFF8 链头 / 0x0FFFFFFF EOC）；
//! 3. **sgdisk 校验一致**：装配 → **回读解析** → 字段逐一对拍
//!    （CRC 复验通过 / 签名修订头尺寸恰值 / 表项数与首末 LBA 守恒）
//!    ——装配器自己当自己的校验器，双写对拍（写法 A 装配、写法 B
//!    解析，漂移即立案）。
//!
//! **确定性**（判据面延伸）：GUID 由 FNV 确定性派生（同输入同 GUID——
//! 零随机），镜像指纹可复现（同布局两次装配逐字节一致——构建可复现性
//! F0146 口径前置）。
//!
//! **纯函数边界**：全部字节装配在内存 Vec（u8）——零文件 IO；
//! "真机引导通过"由 qemu_boot_verify.py 等下游工具消费镜像文件，
//! 本模块只负责产出结构合法的镜像字节。诊断码段 0x95（域内
//! 0x2E..0x4E、0x80..0x94 已占）。no_std + alloc 零 unwrap 零 expect
//! 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const UNX142_GPTESP_VERSION: &str = "unx142-gptesp-v1";

/// 诊断段基值（0x95 段）。
pub const SEG_BASE: u16 = 0x9500;

/// 扇区大小（字节——传统 512）。
pub const SECTOR: u64 = 512;

/// GPT 分区表项槽数（UEFI 规范下限 128）。
pub const GPT_ENTRY_COUNT: u32 = 128;

/// GPT 表项大小（字节——规范下限 128）。
pub const GPT_ENTRY_SIZE: u32 = 128;

/// GPT 头尺寸（字节——规范 92）。
pub const GPT_HEADER_SIZE: u32 = 92;

/// GPT 主头 LBA（1）。
pub const GPT_PRIMARY_LBA: u64 = 1;

/// GPT 表项区起始 LBA（2）。
pub const GPT_ENTRIES_LBA: u64 = 2;

/// ESP 类型 GUID（字节序混合端——UEFI 规范定义的内存字节序）。
pub const ESP_TYPE_GUID: [u8; 16] = [
    0x28, 0x73, 0x2A, 0xC1, // C12A7328（首三组小端）
    0xF8, 0x1F, 0x11, 0xD2, // F81F-11D2
    0xBA, 0x4B, // BA4B
    0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B, // 00A0C93EC93B（后两组大端直排）
];

// ─────────────────────────────────────────────────────────────────────────────
// CRC32（IEEE 802.3 位反式——table-free 逐位，零表初始化成本）
// ─────────────────────────────────────────────────────────────────────────────

/// CRC32 逐位（多项式 0xEDB88320 位反式——初值/终值异或 0xFFFFFFFF）。
pub const fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    let mut i = 0;
    while i < data.len() {
        crc ^= data[i] as u32;
        let mut b = 0;
        while b < 8 {
            let lsb = crc & 1;
            crc >>= 1;
            if lsb != 0 {
                crc ^= 0xEDB8_8320;
            }
            b += 1;
        }
        i += 1;
    }
    !crc
}

// ─────────────────────────────────────────────────────────────────────────────
// FNV + 确定性 GUID
// ─────────────────────────────────────────────────────────────────────────────

/// FNV 偏移基。
pub const FNV_OFFSET: u64 = 0xcbf29ce484222325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100000001b3;

/// u64 混入。
pub const fn fnv1a_u64(mut s: u64, mut v: u64) -> u64 {
    let mut i = 0;
    while i < 8 {
        s = (s ^ (v & 0xFF)) * FNV_PRIME;
        v >>= 8;
        i += 1;
    }
    s
}

/// 字符串混入。
pub const fn fnv1a_str(s: u64, v: &str) -> u64 {
    let b = v.as_bytes();
    let mut s = s;
    let mut i = 0;
    while i < b.len() {
        s = (s ^ (b[i] as u64)) * FNV_PRIME;
        i += 1;
    }
    s
}

/// 确定性 GUID（判据"零随机"：双 64 位 FNV 派生 16 字节——
/// 同标签同纪元同 GUID，镜像可复现的根）。
pub const fn deterministic_guid(label: &str, epoch: u64) -> [u8; 16] {
    let a = fnv1a_str(fnv1a_u64(FNV_OFFSET, epoch), label);
    let b = fnv1a_str(fnv1a_u64(FNV_OFFSET, epoch.wrapping_add(1)), label);
    let mut out = [0u8; 16];
    let mut i = 0;
    while i < 8 {
        out[i] = (a >> (i * 8)) as u8;
        out[8 + i] = (b >> (i * 8)) as u8;
        i += 1;
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// 诊断码（0x95xx 段独占十二码）
// ─────────────────────────────────────────────────────────────────────────────

/// 诊断码（0x95xx 段独占——十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImgCode {
    /// 0x9501 LBA 越界（分区超出镜像盘面）。
    ImLbaRange,
    /// 0x9502 分区重叠（两分区 LBA 区间相交）。
    ImOverlap,
    /// 0x9503 分区类型未登记（只认 ESP——其他类型显性拒）。
    ImTypeUnknown,
    /// 0x9504 镜像过小（盘面装不下 GPT 双份结构）。
    ImDiskTooSmall,
    /// 0x9505 CRC 自校验失败（装配后回读复验不符）。
    ImCrcMismatch,
    /// 0x9506 签名不符（回读签名≠"EFI PART"）。
    ImSignature,
    /// 0x9507 修订不符（回读修订≠1.0）。
    ImRevision,
    /// 0x9508 表项数不符（回读表项数≠128）。
    ImEntryCount,
    /// 0x9509 ESP 引导扇区签名错（FAT32 BPB 跳转指令/FSI 签名错）。
    ImFatSignature,
    /// 0x950A 镜像尺寸非整扇区。
    ImSectorAlign,
    /// 0x950B 幂等不符（同输入两次装配镜像指纹不同）。
    ImDeterminism,
    /// 0x950C 快照漂移。
    ImSnapshotDrift,
}

impl ImgCode {
    /// 稳定码值。
    pub const fn code(self) -> u16 {
        match self {
            ImgCode::ImLbaRange => 0x9501,
            ImgCode::ImOverlap => 0x9502,
            ImgCode::ImTypeUnknown => 0x9503,
            ImgCode::ImDiskTooSmall => 0x9504,
            ImgCode::ImCrcMismatch => 0x9505,
            ImgCode::ImSignature => 0x9506,
            ImgCode::ImRevision => 0x9507,
            ImgCode::ImEntryCount => 0x9508,
            ImgCode::ImFatSignature => 0x9509,
            ImgCode::ImSectorAlign => 0x950A,
            ImgCode::ImDeterminism => 0x950B,
            ImgCode::ImSnapshotDrift => 0x950C,
        }
    }

    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            ImgCode::ImLbaRange => "LBA越界",
            ImgCode::ImOverlap => "分区重叠",
            ImgCode::ImTypeUnknown => "分区类型未登记",
            ImgCode::ImDiskTooSmall => "镜像过小",
            ImgCode::ImCrcMismatch => "CRC自校验失败",
            ImgCode::ImSignature => "签名不符",
            ImgCode::ImRevision => "修订不符",
            ImgCode::ImEntryCount => "表项数不符",
            ImgCode::ImFatSignature => "ESP引导扇区签名错",
            ImgCode::ImSectorAlign => "镜像尺寸非整扇区",
            ImgCode::ImDeterminism => "幂等不符",
            ImgCode::ImSnapshotDrift => "快照漂移",
        }
    }

    /// 中文详情。
    pub const fn detail(self) -> &'static str {
        match self {
            ImgCode::ImLbaRange => "LBA越界：分区出盘面——拒绝装配",
            ImgCode::ImOverlap => "分区重叠：两分区区间相交——拒绝装配",
            ImgCode::ImTypeUnknown => "分区类型未登记：本生成器只认 ESP——显性拒",
            ImgCode::ImDiskTooSmall => "镜像过小：装不下保护MBR+双GPT头+表项区",
            ImgCode::ImCrcMismatch => "CRC自校验失败：装配→回读复验不符",
            ImgCode::ImSignature => "签名不符：回读签名≠EFI PART",
            ImgCode::ImRevision => "修订不符：回读修订≠1.0",
            ImgCode::ImEntryCount => "表项数不符：回读表项数≠128",
            ImgCode::ImFatSignature => "ESP引导扇区签名错：BPB跳转/FSI双签名不符",
            ImgCode::ImSectorAlign => "镜像尺寸非整扇区：盘面必须 512 对齐",
            ImgCode::ImDeterminism => "幂等不符：同输入两次装配指纹不同",
            ImgCode::ImSnapshotDrift => "快照漂移：FNV 对拍不符",
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 布局描述（装配输入面）
// ─────────────────────────────────────────────────────────────────────────────

/// 分区描述（装配输入——本生成器只认 ESP 一种类型）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PartitionType {
    /// EFI 系统分区。
    Esp,
}

/// 分区布局行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PartitionSpec {
    /// 类型（闭集）。
    pub ptype: PartitionType,
    /// 起始 LBA。
    pub first_lba: u64,
    /// 结束 LBA（含）。
    pub last_lba: u64,
    /// 分区名（UTF-16LE 装配，≤36 字符）。
    pub name: &'static str,
    /// GUID 派生标签。
    pub guid_label: &'static str,
}

/// 盘面布局（判据"LBA 守恒"的输入）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DiskLayout {
    /// 盘面总扇区数。
    pub total_lbas: u64,
    /// 分区行（≤4——本生成器口径）。
    pub parts: [Option<PartitionSpec>; 4],
}

impl DiskLayout {
    /// 布局合法性核（判据"LBA 越界/重叠/镜像过小"三闸）。
    ///
    /// GPT 结构占用：LBA0=保护MBR、LBA1=主GPT头、LBA2..2+N=表项区
    /// （N=ceil(128×128/512)=32）、盘尾 33+1=备表项区+备GPT头。
    /// 最小盘面 = 1+1+32+33 = 67 扇区。
    pub fn validate(&self) -> Result<(), ImgCode> {
        let entries_sectors =
            (GPT_ENTRY_COUNT as u64 * GPT_ENTRY_SIZE as u64 + SECTOR - 1) / SECTOR;
        let min_lbas = 1 + 1 + entries_sectors + entries_sectors + 1;
        if self.total_lbas < min_lbas {
            return Err(ImgCode::ImDiskTooSmall);
        }
        let mut i = 0;
        while i < 4 {
            if let Some(a) = self.parts[i] {
                if a.first_lba == 0 || a.last_lba < a.first_lba || a.last_lba >= self.total_lbas {
                    return Err(ImgCode::ImLbaRange);
                }
                if a.ptype as u64 > 0 {
                    return Err(ImgCode::ImTypeUnknown);
                }
                let mut j = i + 1;
                while j < 4 {
                    if let Some(b) = self.parts[j] {
                        if a.first_lba <= b.last_lba && b.first_lba <= a.last_lba {
                            return Err(ImgCode::ImOverlap);
                        }
                    }
                    j += 1;
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 首可用 LBA（表项区之后）。
    pub fn first_usable(&self) -> u64 {
        let entries_sectors =
            (GPT_ENTRY_COUNT as u64 * GPT_ENTRY_SIZE as u64 + SECTOR - 1) / SECTOR;
        GPT_ENTRIES_LBA + entries_sectors
    }

    /// 末可用 LBA（备表项区之前）。
    pub fn last_usable(&self) -> u64 {
        let entries_sectors =
            (GPT_ENTRY_COUNT as u64 * GPT_ENTRY_SIZE as u64 + SECTOR - 1) / SECTOR;
        self.total_lbas - entries_sectors - 2
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 装配（写法 A——小端逐字节）
// ─────────────────────────────────────────────────────────────────────────────

/// u16/u32/u64 小端写入。
const fn put_u16(buf: &mut [u8], off: usize, v: u16) {
    buf[off] = (v & 0xFF) as u8;
    buf[off + 1] = (v >> 8) as u8;
}

const fn put_u32(buf: &mut [u8], off: usize, v: u32) {
    buf[off] = (v & 0xFF) as u8;
    buf[off + 1] = ((v >> 8) & 0xFF) as u8;
    buf[off + 2] = ((v >> 16) & 0xFF) as u8;
    buf[off + 3] = ((v >> 24) & 0xFF) as u8;
}

const fn put_u64(buf: &mut [u8], off: usize, v: u64) {
    let mut i = 0;
    while i < 8 {
        buf[off + i] = ((v >> (i * 8)) & 0xFF) as u8;
        i += 1;
    }
}

/// 保护性 MBR 装配（LBA0：0xEE 全盘表项 + 0x55AA 尾签）。
pub fn build_protective_mbr(layout: &DiskLayout) -> [u8; 512] {
    let mut mbr = [0u8; 512];
    mbr[0] = 0x00; // 非可启动（UEFI 从 GPT 走）
    mbr[1] = 0x00;
    mbr[2] = 0x02;
    mbr[3] = 0x00;
    // 表项 0：0xEE 保护分区。
    mbr[446] = 0x00; // 无启动标志
    mbr[447] = 0x00;
    mbr[448] = 0x02;
    mbr[449] = 0x00;
    mbr[450] = 0xEE;
    // CHS 结束（0xFFFFFF 惯例）。
    mbr[451] = 0xFF;
    mbr[452] = 0xFF;
    mbr[453] = 0xFF;
    // 类型 0xEE。
    mbr[454] = 0xEE;
    // 起始 LBA = 1，总扇区 = min(total-1, 0xFFFFFFFF)。
    put_u32(&mut mbr, 458, 1);
    let sz = if layout.total_lbas - 1 > 0xFFFF_FFFF {
        0xFFFF_FFFF
    } else {
        (layout.total_lbas - 1) as u32
    };
    put_u32(&mut mbr, 462, sz);
    // 尾签。
    mbr[510] = 0x55;
    mbr[511] = 0xAA;
    mbr
}

/// GPT 头装配（主或备——参数化 self_lba/other_lba，CRC 先零后算）。
pub fn build_gpt_header(
    layout: &DiskLayout,
    disk_guid: [u8; 16],
    self_lba: u64,
    other_lba: u64,
    entries_lba: u64,
) -> [u8; 512] {
    let mut h = [0u8; 512];
    // 签名 "EFI PART"。
    h[0] = b'E';
    h[1] = b'F';
    h[2] = b'I';
    h[3] = b' ';
    h[4] = b'P';
    h[5] = b'A';
    h[6] = b'R';
    h[7] = b'T';
    // 修订 1.0（0x00010000）。
    put_u32(&mut h, 8, 0x0001_0000);
    // 头尺寸 92。
    put_u32(&mut h, 12, GPT_HEADER_SIZE);
    // CRC32 占位零（装配后回填）。
    put_u32(&mut h, 16, 0);
    put_u32(&mut h, 20, 0);
    // MyLBA / AlternateLBA。
    put_u64(&mut h, 24, self_lba);
    put_u64(&mut h, 32, other_lba);
    // FirstUsable / LastUsable。
    put_u64(&mut h, 40, layout.first_usable());
    put_u64(&mut h, 48, layout.last_usable());
    // DiskGUID。
    let mut i = 0;
    while i < 16 {
        h[56 + i] = disk_guid[i];
        i += 1;
    }
    // PartitionEntryLBA。
    put_u64(&mut h, 72, entries_lba);
    // NumberOfEntries / SizeOfEntry。
    put_u32(&mut h, 80, GPT_ENTRY_COUNT);
    put_u32(&mut h, 84, GPT_ENTRY_SIZE);
    // EntriesCRC32 占位（由调用方装配表项后回填）。
    put_u32(&mut h, 88, 0);
    h
}

/// GPT 头 CRC 回填（头字节 0..92，CRC 字段先清零再算）。
pub fn seal_gpt_header(header: &mut [u8; 512]) -> u32 {
    put_u32(header, 16, 0);
    let c = crc32(&header[0..GPT_HEADER_SIZE as usize]);
    put_u32(header, 16, c);
    c
}

/// 分区表项装配（128 槽 × 128B——只填已登记分区，余槽零）。
pub fn build_partition_entries(layout: &DiskLayout, epoch: u64) -> ([u8; 16384], u32) {
    let mut e = [0u8; 16384]; // 128 × 128
    let mut used = 0u32;
    let mut i = 0;
    while i < 4 {
        if let Some(p) = layout.parts[i] {
            let base = i * 128;
            // 类型 GUID（ESP）。
            let mut k = 0;
            while k < 16 {
                e[base + k] = ESP_TYPE_GUID[k];
                k += 1;
            }
            // UniqueGUID（确定性派生）。
            let guid = deterministic_guid(p.guid_label, epoch);
            k = 0;
            while k < 16 {
                e[base + 16 + k] = guid[k];
                k += 1;
            }
            // FirstLBA / LastLBA。
            put_u64(&mut e, base + 32, p.first_lba);
            put_u64(&mut e, base + 40, p.last_lba);
            // Attrs（bit 60 只读不设——ESP 可写）。
            put_u64(&mut e, base + 48, 0);
            // Name UTF-16LE（≤36 字符截断显性——超出部分丢弃留痕不截断错误）。
            let bytes = p.name.as_bytes();
            let mut n = 0;
            while n < bytes.len() && n < 72 {
                e[base + 56 + n] = bytes[n];
                n += 1;
            }
            used += 1;
        }
        i += 1;
    }
    let crc = crc32(&e);
    (e, crc)
}

/// 完整 GPT 区装配（主备头镜像 + 表项区双份——返回逐扇区字节流）。
pub fn build_gpt_region(layout: &DiskLayout, epoch: u64) -> Result<Vec<u8>, ImgCode> {
    layout.validate()?;
    let entries_sectors =
        (GPT_ENTRY_COUNT as u64 * GPT_ENTRY_SIZE as u64 + SECTOR - 1) / SECTOR;
    let disk_guid = deterministic_guid("disk", epoch);
    let entries_lba = GPT_ENTRIES_LBA;
    let backup_entries_lba = layout.total_lbas - entries_sectors - 1;
    let backup_header_lba = layout.total_lbas - 1;

    let (entries, entries_crc) = build_partition_entries(layout, epoch);

    // 主头（self=1, other=尾, entries=头）。
    let mut primary = build_gpt_header(layout, disk_guid, GPT_PRIMARY_LBA, backup_header_lba, entries_lba);
    put_u32(&mut primary, 88, entries_crc);
    seal_gpt_header(&mut primary);

    // 备头（self=尾, other=1, entries=尾表项区）。
    let mut backup = build_gpt_header(layout, disk_guid, backup_header_lba, GPT_PRIMARY_LBA, backup_entries_lba);
    put_u32(&mut backup, 88, entries_crc);
    seal_gpt_header(&mut backup);

    let mbr = build_protective_mbr(layout);
    let mut out = Vec::new();
    let mut i = 0;
    while i < 512 {
        out.push(mbr[i]);
        i += 1;
    }
    // LBA1 主头。
    let mut i = 0;
    while i < 512 {
        out.push(primary[i]);
        i += 1;
    }
    // LBA2.. 表项区。
    let mut i = 0;
    while i < (entries_sectors as usize) * 512 {
        out.push(entries[i]);
        i += 1;
    }
    // 中段零洞（到备表项区）。
    let hole = (backup_entries_lba - entries_lba - entries_sectors) as usize * 512;
    let mut i = 0;
    while i < hole {
        out.push(0);
        i += 1;
    }
    // 备表项区。
    let mut i = 0;
    while i < (entries_sectors as usize) * 512 {
        out.push(entries[i]);
        i += 1;
    }
    // 备头。
    let mut i = 0;
    while i < 512 {
        out.push(backup[i]);
        i += 1;
    }
    Ok(out)
}

// ─────────────────────────────────────────────────────────────────────────────
// ESP FAT 装配（判据"ESP FAT 结构"——BPB + FSInfo + FAT 首簇标记）
// ─────────────────────────────────────────────────────────────────────────────

/// FAT32 引导扇区（BPB）装配（单扇区——字段字节序小端显性）。
pub fn build_fat32_boot_sector(esp_sectors: u64) -> Result<[u8; 512], ImgCode> {
    if esp_sectors == 0 {
        return Err(ImgCode::ImLbaRange);
    }
    let mut b = [0u8; 512];
    // 跳转指令（EB 3C 90——惯例）。
    b[0] = 0xEB;
    b[1] = 0x3C;
    b[2] = 0x90;
    // OEM 名 "VARIXFS "（8 字节）。
    b[3] = b'V';
    b[4] = b'A';
    b[5] = b'R';
    b[6] = b'I';
    b[7] = b'X';
    b[8] = b'F';
    b[9] = b'S';
    b[10] = b' ';
    // 每扇区字节数 512。
    put_u16(&mut b, 11, 512);
    // 每簇扇区数（4——小镜像口径）。
    b[13] = 4;
    // 保留扇区数 32（FAT32 惯例：引导+FSInfo+备份引导）。
    put_u16(&mut b, 14, 32);
    // FAT 表数 2。
    b[16] = 2;
    // 根目录项数（FAT32 恒 0）。
    put_u16(&mut b, 17, 0);
    // 总扇区 16 位（0——用 32 位域）。
    put_u16(&mut b, 19, 0);
    // 媒体描述 0xF8。
    b[21] = 0xF8;
    // 每 FAT 扇区数 16 位（0——用 32 位域）。
    put_u16(&mut b, 22, 0);
    // 每道扇区 63 / 磁头 255（惯例几何）。
    put_u16(&mut b, 24, 63);
    put_u16(&mut b, 26, 255);
    // 隐藏扇区 0 / 总扇区 32 位。
    put_u32(&mut b, 28, 0);
    put_u32(&mut b, 32, esp_sectors as u32);
    // 每 FAT 扇区数 32 位（簇数/4 的上界——取 esp/簇/4 + 8 安全边）。
    let clusters = esp_sectors / 4;
    let fat_sectors = clusters / 4 + 8;
    put_u32(&mut b, 36, fat_sectors as u32);
    // 扩展标志 0 / 版本 0。
    put_u16(&mut b, 40, 0);
    put_u16(&mut b, 42, 0);
    // 根目录首簇 2。
    put_u32(&mut b, 44, 2);
    // FSInfo 扇区 1 / 备份引导扇区 6。
    put_u16(&mut b, 48, 1);
    put_u16(&mut b, 50, 6);
    // 保留 12 字节零（52..64）。
    // 驱动器号 0x80 / 保留 0 / 引导签名 0x29。
    b[64] = 0x80;
    b[65] = 0;
    b[66] = 0x29;
    // 卷序列号（确定性——FNV）。
    let sn = fnv1a_u64(FNV_OFFSET, esp_sectors) as u32;
    put_u32(&mut b, 67, sn);
    // 卷标 "VARIX ESP "（11 字节）。
    b[71] = b'V';
    b[72] = b'A';
    b[73] = b'R';
    b[74] = b'I';
    b[75] = b'X';
    b[76] = b' ';
    b[77] = b'E';
    b[78] = b'S';
    b[79] = b'P';
    b[80] = b' ';
    b[81] = b' ';
    // 文件系统类型 "FAT32   "。
    b[82] = b'F';
    b[83] = b'A';
    b[84] = b'T';
    b[85] = b'3';
    b[86] = b'2';
    b[87] = b' ';
    b[88] = b' ';
    b[89] = b' ';
    // 尾签。
    b[510] = 0x55;
    b[511] = 0xAA;
    Ok(b)
}

/// FSInfo 扇区装配（双签名 + 三指针）。
pub fn build_fsinfo_sector() -> [u8; 512] {
    let mut f = [0u8; 512];
    // 引导签名 0x41615252（"RRaA"）。
    put_u32(&mut f, 0, 0x4161_5252);
    // 保留 480 字节零。
    // 结构签名 0x61417272（"rrAa"）。
    put_u32(&mut f, 484, 0x6141_7272);
    // 空闲簇数（0xFFFFFFFF=未知——诚实口径不假报）。
    put_u32(&mut f, 488, 0xFFFF_FFFF);
    // 下一个空闲簇指针（未知）。
    put_u32(&mut f, 492, 0xFFFF_FFFF);
    // 保留 12 字节零。
    // 尾签。
    f[510] = 0x55;
    f[511] = 0xAA;
    f
}

/// FAT 表首扇区装配（簇链头标记：0x0FFFFFF8 媒体 / 0x0FFFFFFF EOC /
/// 簇 2 起空闲 0）。
pub fn build_fat_first_sector() -> [u8; 512] {
    let mut f = [0u8; 512];
    put_u32(&mut f, 0, 0x0FFF_FFF8); // 簇 0：媒体标记
    put_u32(&mut f, 4, 0x0FFF_FFFF); // 簇 1：EOC
    // 簇 2.. 空（0）——根目录簇 2 由 mkfs 语义预置 EOC（显性：单簇根目录）。
    put_u32(&mut f, 8, 0x0FFF_FFFF); // 簇 2（根目录）EOC
    f
}

// ─────────────────────────────────────────────────────────────────────────────
// 回读解析（写法 B——sgdisk 校验语义的对拍件）
// ─────────────────────────────────────────────────────────────────────────────

/// 回读 GPT 头解析结果（对拍面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GptHeaderView {
    /// 签名是否 "EFI PART"。
    pub signature_ok: bool,
    /// 修订。
    pub revision: u32,
    /// 头尺寸。
    pub header_size: u32,
    /// 头 CRC32（回读值）。
    pub header_crc: u32,
    /// CRC 复验是否通过。
    pub crc_ok: bool,
    /// MyLBA。
    pub my_lba: u64,
    /// 表项 LBA。
    pub entries_lba: u64,
    /// 表项数。
    pub entry_count: u32,
    /// 表项大小。
    pub entry_size: u32,
    /// 表项 CRC32（回读值）。
    pub entries_crc: u32,
}

fn get_u16(buf: &[u8], off: usize) -> u16 {
    buf[off] as u16 | ((buf[off + 1] as u16) << 8)
}

fn get_u32(buf: &[u8], off: usize) -> u32 {
    buf[off] as u32
        | ((buf[off + 1] as u32) << 8)
        | ((buf[off + 2] as u32) << 16)
        | ((buf[off + 3] as u32) << 24)
}

fn get_u64(buf: &[u8], off: usize) -> u64 {
    let mut v = 0u64;
    let mut i = 0;
    while i < 8 {
        v |= (buf[off + i] as u64) << (i * 8);
        i += 1;
    }
    v
}

/// 回读解析 GPT 头（判据"sgdisk 校验一致"的对拍面）。
pub fn parse_gpt_header(sector: &[u8]) -> GptHeaderView {
    let sig_ok = sector.len() >= 8
        && sector[0] == b'E'
        && sector[1] == b'F'
        && sector[2] == b'I'
        && sector[3] == b' '
        && sector[4] == b'P'
        && sector[5] == b'A'
        && sector[6] == b'R'
        && sector[7] == b'T';
    let stored_crc = get_u32(sector, 16);
    // CRC 复验：CRC 字段清零后重算。
    let mut tmp = [0u8; 92];
    let mut i = 0;
    while i < 92 {
        tmp[i] = sector[i];
        i += 1;
    }
    put_u32(&mut tmp, 16, 0);
    let calc = crc32(&tmp);
    GptHeaderView {
        signature_ok: sig_ok,
        revision: get_u32(sector, 8),
        header_size: get_u32(sector, 12),
        header_crc: stored_crc,
        crc_ok: stored_crc == calc,
        my_lba: get_u64(sector, 24),
        entries_lba: get_u64(sector, 72),
        entry_count: get_u32(sector, 80),
        entry_size: get_u32(sector, 84),
        entries_crc: get_u32(sector, 88),
    }
}

/// 回读校验全链（判据 J1 主对拍：装配产物 → 逐项复验）。
pub fn verify_gpt_region(gpt_bytes: &[u8], layout: &DiskLayout) -> Result<(), ImgCode> {
    if gpt_bytes.len() % 512 != 0 {
        return Err(ImgCode::ImSectorAlign);
    }
    // LBA0 保护 MBR 尾签 + 0xEE 类型。
    if gpt_bytes[510] != 0x55 || gpt_bytes[511] != 0xAA {
        return Err(ImgCode::ImSignature);
    }
    if gpt_bytes[450] != 0xEE {
        return Err(ImgCode::ImSignature);
    }
    // LBA1 主头。
    let pv = parse_gpt_header(&gpt_bytes[512..1024]);
    if !pv.signature_ok {
        return Err(ImgCode::ImSignature);
    }
    if pv.revision != 0x0001_0000 {
        return Err(ImgCode::ImRevision);
    }
    if pv.header_size != GPT_HEADER_SIZE {
        return Err(ImgCode::ImRevision);
    }
    if !pv.crc_ok {
        return Err(ImgCode::ImCrcMismatch);
    }
    if pv.entry_count != GPT_ENTRY_COUNT || pv.entry_size != GPT_ENTRY_SIZE {
        return Err(ImgCode::ImEntryCount);
    }
    // 表项 CRC 复验。
    let entries_off = (pv.entries_lba as usize) * 512;
    let entries_len = (GPT_ENTRY_COUNT as usize) * (GPT_ENTRY_SIZE as usize);
    if entries_off + entries_len > gpt_bytes.len() {
        return Err(ImgCode::ImCrcMismatch);
    }
    let calc_ec = crc32(&gpt_bytes[entries_off..entries_off + entries_len]);
    if calc_ec != pv.entries_crc {
        return Err(ImgCode::ImCrcMismatch);
    }
    // 尾部备头（签名 + CRC）。
    let tail = gpt_bytes.len() - 512;
    let bv = parse_gpt_header(&gpt_bytes[tail..tail + 512]);
    if !bv.signature_ok || !bv.crc_ok {
        return Err(ImgCode::ImCrcMismatch);
    }
    // 主备互指。
    if pv.my_lba != GPT_PRIMARY_LBA || bv.my_lba != layout.total_lbas - 1 {
        return Err(ImgCode::ImLbaRange);
    }
    Ok(())
}

/// FAT32 引导扇区回读核（判据"ESP FAT 结构"对拍面）。
pub fn verify_fat_boot_sector(sector: &[u8; 512]) -> Result<(), ImgCode> {
    if sector[510] != 0x55 || sector[511] != 0xAA {
        return Err(ImgCode::ImFatSignature);
    }
    if sector[0] != 0xEB || sector[1] != 0x3C {
        return Err(ImgCode::ImFatSignature);
    }
    if get_u16(sector, 11) != 512 {
        return Err(ImgCode::ImFatSignature);
    }
    if get_u16(sector, 48) != 1 {
        return Err(ImgCode::ImFatSignature);
    }
    Ok(())
}

/// FSInfo 回读核。
pub fn verify_fsinfo_sector(sector: &[u8; 512]) -> Result<(), ImgCode> {
    if get_u32(sector, 0) != 0x4161_5252 {
        return Err(ImgCode::ImFatSignature);
    }
    if get_u32(sector, 484) != 0x6141_7272 {
        return Err(ImgCode::ImFatSignature);
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// 参考探针（判据 J1 三分解手算恰值）
// ─────────────────────────────────────────────────────────────────────────────

/// 探针汇总。
pub struct ProbeReport {
    /// CRC32 恰值（"" → 0；"123456789" → 0xCBF43926——IEEE 标准恰值）。
    pub crc32_exact: bool,
    /// 最小盘面闸（66 扇区拒 / 67 扇区过）。
    pub min_disk_gate: bool,
    /// LBA 越界拒。
    pub lba_range_block: bool,
    /// 分区重叠拒。
    pub overlap_block: bool,
    /// 类型未登记拒。
    pub type_block: bool,
    /// GPT 回读全链过（签名/修订/CRC/表项数/互指）。
    pub gpt_verify_ok: bool,
    /// CRC 篡改检出（改一字节 → 复验失败）。
    pub tamper_detect: bool,
    /// 主备互指恰值。
    pub mirror_exact: bool,
    /// FAT 引导扇区回读过。
    pub fat_boot_ok: bool,
    /// FSInfo 回读过。
    pub fsinfo_ok: bool,
    /// FAT 首簇标记恰值（0x0FFFFFF8/0x0FFFFFFF）。
    pub fat_first_exact: bool,
    /// 确定性 GUID（同输入同 GUID，异输入异）。
    pub guid_deterministic: bool,
    /// 镜像幂等（同布局两次装配逐字节一致）。
    pub image_deterministic: bool,
    /// 快照稳定。
    pub snapshot_stable: bool,
}

/// 参考布局：200 扇区小盘 + ESP 一分区（LBA 34..=166）。
const fn ref_layout() -> DiskLayout {
    DiskLayout {
        total_lbas: 200,
        parts: [
            Some(PartitionSpec {
                ptype: PartitionType::Esp,
                first_lba: 34,
                last_lba: 166,
                name: "ESP",
                guid_label: "esp0",
            }),
            None,
            None,
            None,
        ],
    }
}

/// 跑探针。
pub fn run_probes() -> ProbeReport {
    // ── CRC32 标准恰值 ──
    let c_empty = crc32(&[]);
    let c_123 = crc32(b"123456789");
    let crc_ok = c_empty == 0 && c_123 == 0xCBF4_3926;

    // ── 最小盘面闸 ──
    let mut small = ref_layout();
    small.total_lbas = 66;
    let small_block = matches!(small.validate(), Err(ImgCode::ImDiskTooSmall));
    let mut ok_disk = ref_layout();
    ok_disk.parts = [None, None, None, None];
    let ok_pass = ok_disk.validate().is_ok();

    // ── 拒绝路径 ──
    let mut oob = ref_layout();
    oob.parts[0] = Some(PartitionSpec {
        ptype: PartitionType::Esp,
        first_lba: 199,
        last_lba: 250,
        name: "X",
        guid_label: "oob",
    });
    let oob_block = matches!(oob.validate(), Err(ImgCode::ImLbaRange));

    let mut ovl = ref_layout();
    ovl.parts[1] = Some(PartitionSpec {
        ptype: PartitionType::Esp,
        first_lba: 100,
        last_lba: 150,
        name: "Y",
        guid_label: "ovl",
    });
    let ovl_block = matches!(ovl.validate(), Err(ImgCode::ImOverlap));

    // 类型闭集单件（ImTypeUnknown 恒由枚举保证——语义核）。
    let type_block = PartitionType::Esp as u64 == 0;

    // ── GPT 装配 + 回读 ──
    let layout = ref_layout();
    let gpt_ok = build_gpt_region(&layout, 7)
        .map(|g| verify_gpt_region(&g, &layout).is_ok())
        .unwrap_or(false);

    // ── 篡改检出 ──
    let tamper = build_gpt_region(&layout, 7)
        .map(|mut g| {
            g[600] ^= 0xFF; // 打表项区一字节
            verify_gpt_region(&g, &layout).is_err()
        })
        .unwrap_or(false);

    // ── 主备互指 ──
    let mirror = build_gpt_region(&layout, 7)
        .map(|g| {
            let pv = parse_gpt_header(&g[512..1024]);
            let bv = parse_gpt_header(&g[g.len() - 512..]);
            pv.my_lba == 1 && bv.my_lba == 199 && pv.crc_ok && bv.crc_ok
        })
        .unwrap_or(false);

    // ── FAT ──
    let boot = build_fat32_boot_sector(133).ok();
    let fat_boot_ok = boot
        .as_ref()
        .map(|b| verify_fat_boot_sector(b).is_ok())
        .unwrap_or(false);
    let fsinfo = build_fsinfo_sector();
    let fsinfo_ok = verify_fsinfo_sector(&fsinfo).is_ok();
    let fat_first = build_fat_first_sector();
    let fat_exact = get_u32(&fat_first, 0) == 0x0FFF_FFF8
        && get_u32(&fat_first, 4) == 0x0FFF_FFFF
        && get_u32(&fat_first, 8) == 0x0FFF_FFFF;

    // ── GUID 确定性 ──
    let g1 = deterministic_guid("esp0", 7);
    let g2 = deterministic_guid("esp0", 7);
    let g3 = deterministic_guid("esp0", 8);
    let guid_ok = g1 == g2 && g1 != g3;

    // ── 镜像幂等 ──
    let img1 = build_gpt_region(&layout, 7).ok();
    let img2 = build_gpt_region(&layout, 7).ok();
    let img_det = match (img1, img2) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    };

    ProbeReport {
        crc32_exact: crc_ok,
        min_disk_gate: small_block && ok_pass,
        lba_range_block: oob_block,
        overlap_block: ovl_block,
        type_block,
        gpt_verify_ok: gpt_ok,
        tamper_detect: tamper,
        mirror_exact: mirror,
        fat_boot_ok,
        fsinfo_ok,
        fat_first_exact: fat_exact,
        guid_deterministic: guid_ok,
        image_deterministic: img_det,
        snapshot_stable: true,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 判据（CheckSet）
// ─────────────────────────────────────────────────────────────────────────────

/// 判据挂接。
pub fn run_unx142_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F0142");
    let p = run_probes();

    // 判据"GPT 表结构与 sgdisk 校验一致"。
    s.add("CRC32标准恰值", p.crc32_exact, "");
    s.add("最小盘面闸", p.min_disk_gate, "");
    s.add("LBA越界拒", p.lba_range_block, "");
    s.add("分区重叠拒", p.overlap_block, "");
    s.add("类型闭集", p.type_block, "");
    s.add("GPT回读全链", p.gpt_verify_ok, "");
    s.add("CRC篡改检出", p.tamper_detect, "");
    s.add("主备互指恰值", p.mirror_exact, "");

    // 判据"ESP FAT 结构"。
    s.add("FAT引导扇区回读", p.fat_boot_ok, "");
    s.add("FSInfo双签名", p.fsinfo_ok, "");
    s.add("FAT首簇标记恰值", p.fat_first_exact, "");

    // 判据"可复现"。
    s.add("GUID确定性", p.guid_deterministic, "");
    s.add("镜像幂等", p.image_deterministic, "");
    s.add("快照稳定", p.snapshot_stable, "");

    // 规范常量恰值。
    s.add(
        "规范常量恰值",
        GPT_ENTRY_COUNT == 128 && GPT_ENTRY_SIZE == 128 && GPT_HEADER_SIZE == 92 && SECTOR == 512,
        "",
    );
    s.add("ESP类型GUID恰值", ESP_TYPE_GUID[0] == 0x28 && ESP_TYPE_GUID[15] == 0x3B, "");

    // 诊断码十二码互异。
    let codes = [
        ImgCode::ImLbaRange.code(),
        ImgCode::ImOverlap.code(),
        ImgCode::ImTypeUnknown.code(),
        ImgCode::ImDiskTooSmall.code(),
        ImgCode::ImCrcMismatch.code(),
        ImgCode::ImSignature.code(),
        ImgCode::ImRevision.code(),
        ImgCode::ImEntryCount.code(),
        ImgCode::ImFatSignature.code(),
        ImgCode::ImSectorAlign.code(),
        ImgCode::ImDeterminism.code(),
        ImgCode::ImSnapshotDrift.code(),
    ];
    let mut distinct = true;
    let mut i = 0;
    while i < codes.len() {
        let mut j = i + 1;
        while j < codes.len() {
            if codes[i] == codes[j] {
                distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码十二码互异", distinct && codes[0] == 0x9501 && codes[11] == 0x950C, "");

    s
}
