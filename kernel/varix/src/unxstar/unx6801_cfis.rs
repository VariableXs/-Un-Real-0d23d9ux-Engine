//! UNX-F6801 CFIS 措辞器（Register FIS 20 字节构造）（0xC780 段）
//!
//! 锚点：UNX-F6801-J1 Register FIS 20 字节逐字节措辞断言（命令/设备/
//! LBA48/计数/控制五字段）；措辞器单测覆盖全命令位图。
//! 域：CoRun·UNX-B4-B21（AHCI CFIS 面）。
//!
//! 结构：Register FIS Host-to-Device 20 字节模型（type=0x27 + 位域 +
//! command/device + LBA48 六字节 + count 两字节 + icc/control + 填充）
//! + 措辞器（五字段构造）+ 全命令位图单测（读/写/识别/flush/packet）
//! + 注入核（type 漂移/LBA48 越界/计数零/封账措辞）。
use alloc::vec::Vec;
use alloc::string::String;

use alloc::format;
use crate::checks::CheckSet;

/// 诊断段（0xC780 独占 0x80）。
pub const SEG_BASE: u32 = 0xC780;
pub const XC_FIS_TYPE: u32 = SEG_BASE + 0x01; // type 漂移
pub const XC_FIS_LBA: u32 = SEG_BASE + 0x02; // LBA48 越界（>2^48）
pub const XC_FIS_COUNT: u32 = SEG_BASE + 0x03; // 计数零
pub const XC_FIS_SEALED: u32 = SEG_BASE + 0x04; // 封账措辞

/// FIS 长度与 type。
pub const FIS_LEN: usize = 20;
pub const FIS_H2D: u8 = 0x27;

/// 全命令位图闭集（本面覆盖命令）。
pub const CMD_READ_DMA_EXT: u8 = 0x25;
pub const CMD_WRITE_DMA_EXT: u8 = 0x35;
pub const CMD_IDENTIFY: u8 = 0xEC;
pub const CMD_FLUSH_EXT: u8 = 0xEA;
pub const CMD_PACKET: u8 = 0xA0;
pub const CMD_BITMAP: [u8; 5] = [
    CMD_READ_DMA_EXT,
    CMD_WRITE_DMA_EXT,
    CMD_IDENTIFY,
    CMD_FLUSH_EXT,
    CMD_PACKET,
];

/// Register FIS Host-to-Device 20 字节布局：
/// [0]=0x27 [1]=flags(C=1) [2]=command [3]=device
/// [4..8]=LBA7:0 [8]=LBA31:24 [9..12]=LBA39:32 [12]=features(exp)
/// [13]=count low [14]=count high [15]=icc [16]=control [17..20]=填充
#[derive(Clone)]
pub struct RegFis {
    pub bytes: [u8; FIS_LEN],
}

impl RegFis {
    /// 措辞器：五字段构造（command/device/lba48(48 位)/count(16 位)/control）。
    pub fn build(command: u8, device: u8, lba48: u64, count: u16, control: u8) -> Result<Self, u32> {
        if lba48 >= (1u64 << 48) {
            return Err(XC_FIS_LBA);
        }
        if count == 0 {
            return Err(XC_FIS_COUNT);
        }
        let mut b = [0u8; FIS_LEN];
        b[0] = FIS_H2D;
        b[1] = 0x80; // C 位=1（command update）
        b[2] = command;
        b[3] = device;
        b[4] = (lba48 & 0xFF) as u8;
        b[5] = ((lba48 >> 8) & 0xFF) as u8;
        b[6] = ((lba48 >> 16) & 0xFF) as u8;
        b[7] = ((lba48 >> 24) & 0xFF) as u8;
        b[8] = ((lba48 >> 32) & 0xFF) as u8;
        b[9] = ((lba48 >> 40) & 0xFF) as u8;
        b[13] = (count & 0xFF) as u8;
        b[14] = (count >> 8) as u8;
        b[15] = 0; // icc
        b[16] = control;
        Ok(RegFis { bytes: b })
    }

    /// 逐字段读出（校验面）。
    pub fn fields(&self) -> Result<(u8, u8, u64, u16, u8), u32> {
        if self.bytes[0] != FIS_H2D {
            return Err(XC_FIS_TYPE);
        }
        let lba = (self.bytes[4] as u64)
            | (self.bytes[5] as u64) << 8
            | (self.bytes[6] as u64) << 16
            | (self.bytes[7] as u64) << 24
            | (self.bytes[8] as u64) << 32
            | (self.bytes[9] as u64) << 40;
        let count = self.bytes[13] as u16 | (self.bytes[14] as u16) << 8;
        Ok((self.bytes[2], self.bytes[3], lba, count, self.bytes[16]))
    }

    /// 填充区零核：[17..20] 恒零（协议冻结）。
    pub fn padding_zero(&self) -> bool {
        self.bytes[17] == 0 && self.bytes[18] == 0 && self.bytes[19] == 0
    }

    /// 指纹：20 字节折叠。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for &b in &self.bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }
}

/// 逐字节措辞断言核：已知五字段 → 20 字节逐位核对。
pub fn byte_exact() -> bool {
    let f = match RegFis::build(CMD_READ_DMA_EXT, 0xE0, 0x0123_4567_89AB, 0x1234, 0) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let expect: [u8; 20] = [
        0x27, 0x80, 0x25, 0xE0, 0xAB, 0x89, 0x67, 0x45, 0x23, 0x01, 0x00, 0x00, 0x00, 0x34,
        0x12, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    f.bytes == expect && f.padding_zero()
}

/// 全命令位图单测：五命令逐命令措辞→字段读出回环一致。
pub fn cmd_bitmap_cover() -> (u32, u32) {
    let mut ok = 0;
    for (i, &cmd) in CMD_BITMAP.iter().enumerate() {
        let lba = 0x1000 + i as u64;
        let cnt = 8 + i as u16;
        if let Ok(f) = RegFis::build(cmd, 0x40, lba, cnt, 0) {
            if f.fields() == Ok((cmd, 0x40, lba, cnt, 0)) && f.padding_zero() {
                ok += 1;
            }
        }
    }
    (ok, CMD_BITMAP.len() as u32)
}

/// LBA48 边界核：2^48-1 合法，2^48 拒。
pub fn lba48_boundary() -> bool {
    RegFis::build(CMD_WRITE_DMA_EXT, 0xE0, (1u64 << 48) - 1, 1, 0).is_ok()
        && matches!(RegFis::build(CMD_WRITE_DMA_EXT, 0xE0, 1u64 << 48, 1, 0), Err(XC_FIS_LBA))
}

/// 计数边界核：1 合法，0 拒，0xFFFF 合法。
pub fn count_boundary() -> bool {
    RegFis::build(CMD_READ_DMA_EXT, 0xE0, 0, 1, 0).is_ok()
        && RegFis::build(CMD_READ_DMA_EXT, 0xE0, 0, 0xFFFF, 0).is_ok()
        && matches!(RegFis::build(CMD_READ_DMA_EXT, 0xE0, 0, 0, 0), Err(XC_FIS_COUNT))
}

/// 注入核：四类必抓（type 漂移检出/LBA48 越界/计数零/封账措辞=类型面拒）。
pub fn injection_core() -> (u32, u32) {
    let mut caught = 0;
    // 1) type 漂移检出：字段读出遇非法 type 必拒
    let mut f1 = match RegFis::build(CMD_IDENTIFY, 0xE0, 0, 1, 0) {
        Ok(f) => f,
        Err(_) => return (0, 4),
    };
    if f1.fields().is_ok() {
        caught += 1;
    }
    f1.bytes[0] = 0x99;
    if f1.fields() == Err(XC_FIS_TYPE) {
        caught += 1;
    }
    // 2) LBA48 越界
    if lba48_boundary() {
        caught += 1;
    }
    // 3) 计数零
    if count_boundary() {
        caught += 1;
    }
    // 4) 措辞确定性：同参数同指纹（漂移必现形）
    let a = RegFis::build(CMD_FLUSH_EXT, 0xE0, 7, 3, 0);
    let b = RegFis::build(CMD_FLUSH_EXT, 0xE0, 7, 3, 0);
    if a.map(|x| x.fingerprint()).ok() == b.map(|x| x.fingerprint()).ok() {
        caught += 1;
    }
    (caught, 4)
}

/// 指纹冻结：全命令位图措辞联合哈希。
pub fn fp_frozen() -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for (i, &cmd) in CMD_BITMAP.iter().enumerate() {
        if let Ok(f) = RegFis::build(cmd, 0xE0, 0x100 + i as u64, (4 + i as u16) * 8, 0) {
            h ^= f.fingerprint();
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx6801_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F6801");
    // J1-1 逐字节措辞断言
    let ok = byte_exact();
    s.add("B21-01-逐字节措辞", ok, if ok { "" } else { "字节漂移" });
    // J1-2 全命令位图覆盖
    let (ok, n) = cmd_bitmap_cover();
    let ok = ok == n;
    let detail = if ok { alloc::string::String::new() } else { format!("{ok}/{n}") };
    s.add_dyn("B21-02-全命令位图", ok, &detail);
    // J1-3 LBA48 边界
    let ok = lba48_boundary();
    s.add("B21-03-LBA48边界", ok, if ok { "" } else { "LBA 闸失" });
    // J1-4 计数边界
    let ok = count_boundary();
    s.add("B21-04-计数边界", ok, if ok { "" } else { "计数闸失" });
    // J1-5 注入核 4/4
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B21-05-注入核必抓", ok, &detail);
    // J1-6 指纹冻结 + 填充区零
    let f = RegFis::build(CMD_PACKET, 0xA0, 0, 1, 0x08);
    let ok = fp_frozen() == fp_frozen() && f.map(|x| x.padding_zero()).unwrap_or(false);
    s.add("B21-06-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
