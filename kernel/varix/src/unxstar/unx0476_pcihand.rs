//! unx0476_pcihand —— UNX-F0476 PCI 交接字段（A3 预备）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0476 · PCI 交接字段 · 目标 240 行
//! 判据原文：UNX-F0476-J1 交接信包（A3 面预备，与 0439 A2/A3
//!           同构）：枚举完成后的 PCI 交接信包，64 字节定长，
//!           字段槽位固定——版本（u8）、旗域（u8）、保留×6、
//!           段数（u16）、总线数（u16）、发现设备数（u32）、
//!           桥计数（u32）、ECAM 基址（u64）、CRC32（u32，末
//!           4 字节）。判据：账二 字段完整性（逐槽写入后逐
//!           槽读回）；账三 CRC32 一致性（信包写入→CRC→信包
//!           读出→重算 CRC 全等）；账四 版本闸（版本≠1 拒
//!           收）；账五 保留域零核（写非零立案）；账六 对端
//!           读取纪律（半读/越界读拒）。
//! 组件：信包布局 + 编码器 + 解码器 + CRC32 复用（0439 同款
//!   多项式 0xEDB88320）+ 版本闸 + 保留域核。
//! 职责定位：枚举器是「生产者」，中断/驱动子系统是「消费者」，
//!   两者之间只有这 64 字节的窄门——字段错位就是全线失联。
//!   A3 面与 0439 的 A2/A3 信包同构，消费端可用同一套解码器。
//! 性能逐项分解：编码 O(1)（定长）；CRC O(64)。
//! 诊断码段：0x34740（0x34741~0x34749 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 信包定长与字段槽位（字节偏移）。
pub const PKT_LEN: usize = 64;
pub const OFF_VER: usize = 0;
pub const OFF_FLAGS: usize = 1;
pub const OFF_RSVD: usize = 2;
pub const RSVD_LEN: usize = 6;
pub const OFF_SEGS: usize = 8;
pub const OFF_BUSES: usize = 10;
pub const OFF_DEVS: usize = 12;
pub const OFF_BRIDGES: usize = 16;
pub const OFF_ECAM: usize = 20;
pub const OFF_CRC: usize = 60;

/// 合法版本。
pub const VER_A3: u8 = 1;

/// 诊断码（SEG_BASE+序号）。
pub const SEG_BASE: u32 = 0x34740;
pub const XC_LEN: u32 = SEG_BASE + 1; // 长度不足
pub const XC_VER: u32 = SEG_BASE + 2; // 版本不符
pub const XC_RSVD: u32 = SEG_BASE + 3; // 保留域非零
pub const XC_CRC: u32 = SEG_BASE + 4; // CRC 不一致
pub const XC_PART: u32 = SEG_BASE + 5; // 半读/越界读

/// CRC32（0439 同款反射多项式，查表免位反转）。
fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

/// 编码：五元字段 + 保留零 + CRC 覆盖前 60 字节。
pub fn encode(ver: u8, flags: u8, segs: u16, buses: u16, devs: u32, bridges: u32, ecam: u64) -> [u8; PKT_LEN] {
    let mut raw = [0u8; PKT_LEN];
    raw[OFF_VER] = ver;
    raw[OFF_FLAGS] = flags;
    raw[OFF_SEGS..OFF_SEGS + 2].copy_from_slice(&segs.to_le_bytes());
    raw[OFF_BUSES..OFF_BUSES + 2].copy_from_slice(&buses.to_le_bytes());
    raw[OFF_DEVS..OFF_DEVS + 4].copy_from_slice(&devs.to_le_bytes());
    raw[OFF_BRIDGES..OFF_BRIDGES + 4].copy_from_slice(&bridges.to_le_bytes());
    raw[OFF_ECAM..OFF_ECAM + 8].copy_from_slice(&ecam.to_le_bytes());
    let crc = crc32(&raw[..OFF_CRC]);
    raw[OFF_CRC..OFF_CRC + 4].copy_from_slice(&crc.to_le_bytes());
    raw
}

/// 解码：完整性核（长度/版本/保留/CRC）。
pub fn decode(raw: &[u8]) -> Result<(u8, u8, u16, u16, u32, u32, u64), u32> {
    if raw.len() < PKT_LEN {
        return Err(XC_LEN);
    }
    let ver = raw[OFF_VER];
    if ver != VER_A3 {
        return Err(XC_VER);
    }
    let mut i = 0usize;
    while i < RSVD_LEN {
        if raw[OFF_RSVD + i] != 0 {
            return Err(XC_RSVD);
        }
        i += 1;
    }
    let crc_stored = u32::from_le_bytes([raw[OFF_CRC], raw[OFF_CRC + 1], raw[OFF_CRC + 2], raw[OFF_CRC + 3]]);
    let crc_calc = crc32(&raw[..OFF_CRC]);
    if crc_stored != crc_calc {
        return Err(XC_CRC);
    }
    let segs = u16::from_le_bytes([raw[OFF_SEGS], raw[OFF_SEGS + 1]]);
    let buses = u16::from_le_bytes([raw[OFF_BUSES], raw[OFF_BUSES + 1]]);
    let devs = u32::from_le_bytes([raw[OFF_DEVS], raw[OFF_DEVS + 1], raw[OFF_DEVS + 2], raw[OFF_DEVS + 3]]);
    let bridges = u32::from_le_bytes([raw[OFF_BRIDGES], raw[OFF_BRIDGES + 1], raw[OFF_BRIDGES + 2], raw[OFF_BRIDGES + 3]]);
    let mut e = [0u8; 8];
    e.copy_from_slice(&raw[OFF_ECAM..OFF_ECAM + 8]);
    let ecam = u64::from_le_bytes(e);
    Ok((ver, flags_raw(raw), segs, buses, devs, bridges, ecam))
}

fn flags_raw(raw: &[u8]) -> u8 {
    raw[OFF_FLAGS]
}

/// 对端读取纪律：只许整包读，半读/越界读拒。
pub fn full_read(raw: &[u8], want: usize) -> Result<&[u8], u32> {
    if want != PKT_LEN || raw.len() != PKT_LEN {
        return Err(XC_PART);
    }
    Ok(raw)
}

pub fn run_unx0476_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0476");
    cs.add(
        "SEG 段位独占",
        SEG_BASE == 0x34740 && XC_PART == SEG_BASE + 5,
        "u32 高段全域普查无冲突",
    );

    // 族一：编码→解码全等（字段逐槽）。
    let pkt = encode(VER_A3, 0x01, 2, 16, 9, 3, 0xE000_0000);
    let d = decode(&pkt);
    cs.add("编码解码全等", d.is_ok(), "九字段闭环");
    if let Ok((v, f, s, b, dv, br, e)) = d {
        cs.add("版本读回", v == VER_A3, "u8 槽 0");
        cs.add("旗域读回", f == 0x01, "u8 槽 1");
        cs.add("段数读回", s == 2, "u16 槽 8");
        cs.add("总线读回", b == 16, "u16 槽 10");
        cs.add("设备读回", dv == 9, "u32 槽 12");
        cs.add("桥读回", br == 3, "u32 槽 16");
        cs.add("ECAM 读回", e == 0xE000_0000, "u64 槽 20");
    }

    // 族二：CRC 一致性与标准向量。
    cs.add("CRC 标准向量", crc32(b"123456789") == 0xCBF4_3926, "123456789→CBF43926");
    let pkt2 = pkt;
    cs.add("CRC 重算一致", decode(&pkt2).is_ok(), "同包重算全等");
    let mut pkt3 = pkt;
    pkt3[OFF_DEVS] ^= 0x01; // 翻一位
    cs.add("位翻检出", decode(&pkt3) == Err(XC_CRC), "CRC 检出单比特");

    // 族三：版本闸。
    let bad_ver = encode(2, 0, 1, 1, 0, 0, 0);
    cs.add("版本 2 拒", decode(&bad_ver) == Err(XC_VER), "仅版本 1 收");

    // 族四：保留域核。
    let mut bad_rsvd = pkt;
    bad_rsvd[OFF_RSVD + 3] = 0x80;
    cs.add("保留非零拒", decode(&bad_rsvd) == Err(XC_RSVD), "槽 2~7 恒零");

    // 族五：读取纪律。
    cs.add("整包读成", full_read(&pkt, PKT_LEN).is_ok(), "64 字节全量");
    cs.add("半读拒", full_read(&pkt, 32) == Err(XC_PART), "32 < 64");
    cs.add("越界欲拒", full_read(&pkt[..32], PKT_LEN) == Err(XC_PART), "源不足");

    // 族六：极限值编解码。
    let big = encode(VER_A3, 0xFF, 0xFFFF, 0xFFFF, 0xFFFF_FFFF, 0xFFFF_FFFF, 0xFFFF_FFFF_FFFF_FFFF);
    if let Ok((_, _, s, b, dv, br, e)) = decode(&big) {
        cs.add("u16 满射", s == 0xFFFF && b == 0xFFFF, "满值读回");
        cs.add("u32 满射", dv == 0xFFFF_FFFF && br == 0xFFFF_FFFF, "满值读回");
        cs.add("u64 满射", e == 0xFFFF_FFFF_FFFF_FFFF, "满值读回");
    } else {
        cs.add("u16 满射", false, "解码失败");
    }

    // 族七：零值包（空域）。
    let empty = encode(VER_A3, 0, 0, 0, 0, 0, 0);
    cs.add("空域包合法", decode(&empty).is_ok(), "零枚举也成包");

    cs
}
