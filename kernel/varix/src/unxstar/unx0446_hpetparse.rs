//! unx0446_hpetparse —— UNX-F0446 HPET 表解析（F0404 对照兑现）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0446 · HPET 表解析 · 目标 240 行
//! 判据原文：UNX-F0446-J1 HPET 表（签名 "HPET"）解析：事件
//!           定时器块（38 字节）：事件定时器 ID u32、基址
//!           （地址结构：空间 ID u8 + 位宽 u8 + 偏移 u8 +
//!           访问宽 u8 + 地址 u64）、编号 u8（0~31）、最小
//!           周期 u16（ns）、页保护 u8；解析产出三量：HPET
//!           物理基址（空间 ID 0 = 内存态，地址即 MMIO 基）、
//!           定时器编号（n+1 = 比较器数）、最小周期（ Femto
//!           换算核：u16 ns 字段实际单位 100fs，值 v → 周期
//!           = v × 100 fs）；序号核：编号 ≤31；编号 0 主计
//!           时器专位核（ID 位 15 置 = legacy 替换 8254/RTC）；
//!           F0404 对照：解析基址须与 F0404 时钟账登记一致
//!           （本单用软对照桩：登记值由判据传入比）。
//! 组件：事件定时器块拆解器 + 地址结构解码 + 100fs 换算 +
//!       序号域核 + legacy 位核 + F0404 对照桩。
//! 职责定位：HPET 是高精度计时兜底（TSC 不可信时的救生艇），
//!   解析错基址则读写 MMIO 落空，错周期则节拍全歪；100fs
//!   单位换算是 ACPI 规范的著名坑（字段名叫 minimum tick
//!   但单位不是 ns）；legacy 位决定要不要把 PIT/RTC 让位。
//! 性能逐项分解：块拆解 O(1)；换算 O(1)；对照 O(1)。
//! 诊断码段：0x340C0（0x340C1~0x340C9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const HP_VERSION: &str = "unx0446_hpetparse-v1";

/// 诊断段基址（0x340C0 段 · 本模块独占）。
pub const HP_SEG_BASE: u32 = 0x340C0;
pub const XC_SIG: u32 = 0x340C1; // 签名非 HPET
pub const XC_HDR_SHORT: u32 = 0x340C2; // 事件块不足 38
pub const XC_NUM_OOR: u32 = 0x340C3; // 编号 >31
pub const XC_SPACE: u32 = 0x340C4; // 地址空间非内存态
pub const XC_PERIOD0: u32 = 0x340C5; // 最小周期 0
pub const XC_MAGIC: u32 = 0x340C9; // 账魔数破

/// 事件定时器块长（头 40 + 块 38 → 条目区自 40 起）。
pub const HPET_BODY: usize = 38;
/// HPET 表总头（签名+长+...）至事件块：ACPI 头 36。
pub const HPET_HDR: usize = 36;
/// 事件定时器块内偏移。
pub const OFF_HT_ID: usize = 0; // u32 事件定时器 ID
pub const OFF_HT_ADDR: usize = 4; // 12 字节地址结构
pub const OFF_HT_NUM: usize = 16; // u8 编号
pub const OFF_HT_MINPER: usize = 25; // u16 最小周期（100fs 单位）
pub const OFF_HT_PROT: usize = 27; // u8 页保护
/// 100fs → fs。
pub const FS_UNIT: u64 = 100;

/// 解析产出。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HpetInfo {
    /// 事件定时器 ID（低 15 位厂商，位 15 legacy 替换位，位
    /// 16~31 修订号）。
    pub ht_id: u32,
    /// HPET MMIO 物理基址。
    pub base: u64,
    /// 定时器编号（比较器数 = n+1）。
    pub num: u8,
    /// 最小周期（fs）。
    pub min_period_fs: u64,
    /// 页保护。
    pub prot: u8,
}

impl HpetInfo {
    /// 比较器总数。
    pub fn comparators(&self) -> u32 {
        self.num as u32 + 1
    }

    /// legacy 替换位（ID 位 15）：置位 = 8254/RTC 让位。
    pub fn legacy_replaces(&self) -> bool {
        self.ht_id & (1 << 15) != 0
    }

    /// 修订号（ID 位 16~31）。
    pub fn rev(&self) -> u16 {
        (self.ht_id >> 16) as u16
    }
}

/// HPET 表解析。
pub fn parse_hpet(raw: &[u8]) -> Result<HpetInfo, u32> {
    if raw.len() < 4 || &raw[0..4] != b"HPET" {
        return Err(XC_SIG);
    }
    if raw.len() < HPET_HDR + HPET_BODY {
        return Err(XC_HDR_SHORT);
    }
    let b = &raw[HPET_HDR..HPET_HDR + HPET_BODY];
    let ht_id = u32::from_le_bytes([b[OFF_HT_ID], b[OFF_HT_ID + 1], b[OFF_HT_ID + 2], b[OFF_HT_ID + 3]]);
    // 地址结构：空间 ID @0，位宽 @1，偏移 @2，访问宽 @3，地址 u64 @4。
    let space = b[OFF_HT_ADDR];
    if space != 0 {
        return Err(XC_SPACE); // 仅内存态（System Memory）受支持
    }
    let base = u64::from_le_bytes([
        b[OFF_HT_ADDR + 4], b[OFF_HT_ADDR + 5], b[OFF_HT_ADDR + 6], b[OFF_HT_ADDR + 7],
        b[OFF_HT_ADDR + 8], b[OFF_HT_ADDR + 9], b[OFF_HT_ADDR + 10], b[OFF_HT_ADDR + 11],
    ]);
    let num = b[OFF_HT_NUM];
    if num > 31 {
        return Err(XC_NUM_OOR);
    }
    let mp = u16::from_le_bytes([b[OFF_HT_MINPER], b[OFF_HT_MINPER + 1]]) as u64;
    if mp == 0 {
        return Err(XC_PERIOD0);
    }
    Ok(HpetInfo {
        ht_id,
        base,
        num,
        min_period_fs: mp * FS_UNIT,
        prot: b[OFF_HT_PROT],
    })
}

/// 100fs 换算独立核（规范单位坑）。
pub fn period_fs(v: u16) -> u64 {
    v as u64 * FS_UNIT
}

/// F0404 对照桩：解析基址 vs 登记基址。
pub fn crosscheck(info: &HpetInfo, registered: u64) -> Result<(), u32> {
    if info.base == registered {
        Ok(())
    } else {
        Err(XC_MAGIC) // 借魔数段作失配码（对照失配=账面破）
    }
}

/// 造 HPET 表（夹具）。
pub fn mk_hpet(ht_id: u32, base: u64, num: u8, minper: u16) -> Vec<u8> {
    let mut raw = vec![0u8; HPET_HDR + HPET_BODY];
    raw[0..4].copy_from_slice(b"HPET");
    let total = raw.len() as u32;
    raw[4..8].copy_from_slice(&total.to_le_bytes());
    let b = &mut raw[HPET_HDR..];
    b[OFF_HT_ID..OFF_HT_ID + 4].copy_from_slice(&ht_id.to_le_bytes());
    b[OFF_HT_ADDR] = 0; // 内存态
    b[OFF_HT_ADDR + 4..OFF_HT_ADDR + 12].copy_from_slice(&base.to_le_bytes());
    b[OFF_HT_NUM] = num;
    b[OFF_HT_MINPER..OFF_HT_MINPER + 2].copy_from_slice(&minper.to_le_bytes());
    raw
}

/// 判据入口：UNX-F0446。
pub fn run_unx0446_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0446_hpetparse");
    // 族一：基本解析。
    let h = mk_hpet(0x8086_A001, 0xFED0_0000, 2, 10);
    let p = parse_hpet(&h);
    cs.add("基址 FED00000", p.as_ref().map(|x| x.base == 0xFED0_0000).unwrap_or(false), "内存态");
    cs.add("编号 2", p.as_ref().map(|x| x.num).unwrap_or(9) == 2, "比较器 3");
    cs.add("比较器 n+1", p.as_ref().map(|x| x.comparators() == 3).unwrap_or(false), "差一还原");
    cs.add("周期 1000fs", p.as_ref().map(|x| x.min_period_fs == 1000).unwrap_or(false), "10×100fs");
    // 族二：legacy 位。
    cs.add("legacy 置", p.as_ref().map(|x| x.legacy_replaces()).unwrap_or(false), "位 15");
    cs.add("修订 A001", p.as_ref().map(|x| x.rev() == 0x8086).unwrap_or(false), "位 16~31");
    let no_leg = parse_hpet(&mk_hpet(0x1, 0xFED0_0000, 0, 10));
    cs.add("legacy 清不替换", no_leg.map(|x| !x.legacy_replaces()).unwrap_or(false), "位 15 清");
    // 族三：序号域。
    cs.add("编号 31 过", parse_hpet(&mk_hpet(0, 0xFED0_0000, 31, 10)).is_ok(), "域顶");
    cs.add("编号 32 拒", parse_hpet(&mk_hpet(0, 0xFED0_0000, 32, 10)) == Err(XC_NUM_OOR), "越域");
    cs.add("编号 0 单比较器", parse_hpet(&mk_hpet(0, 0xFED0_0000, 0, 10)).map(|x| x.comparators() == 1).unwrap_or(false), "最少 1");
    // 族四：空间域。
    let mut bad_sp = mk_hpet(0, 0xFED0_0000, 2, 10);
    bad_sp[HPET_HDR + OFF_HT_ADDR] = 1; // I/O 态
    cs.add("I/O 态拒", parse_hpet(&bad_sp) == Err(XC_SPACE), "仅内存态");
    // 族五：周期零。
    cs.add("周期 0 拒", parse_hpet(&mk_hpet(0, 0xFED0_0000, 2, 0)) == Err(XC_PERIOD0), "无周期无节拍");
    // 族六：签名/短头。
    cs.add("非 HPET 拒", parse_hpet(b"APICxxxx") == Err(XC_SIG), "签名闸");
    cs.add("短表拒", parse_hpet(b"HPET") == Err(XC_HDR_SHORT), "38 块不足");
    // 族七：换算核。
    cs.add("1→100fs", period_fs(1) == 100, "单位坑");
    cs.add("100→10_000fs", period_fs(100) == 10_000, "十倍");
    cs.add("1000ns 表观 = 100_000fs", period_fs(1000) == 100_000, "0.1ns 实际");
    // 族八：F0404 对照。
    let info = parse_hpet(&mk_hpet(0, 0xFED0_0000, 2, 10)).ok().unwrap_or_else(|| HpetInfo { ht_id: 0, base: 0, num: 0, min_period_fs: 0, prot: 0 });
    cs.add("对照一致", crosscheck(&info, 0xFED0_0000).is_ok(), "登记同址");
    cs.add("对照失配", crosscheck(&info, 0xFED0_1000).is_err(), "异址立案");
    // 族九：典型数值面（ICH 系：基址 FED00000、比较器 3、最小周期 10 → 1ns）。
    let ich = parse_hpet(&mk_hpet(0x8086_A001, 0xFED0_0000, 2, 10));
    cs.add("ICH 面三量", ich.as_ref().map(|x| x.base == 0xFED0_0000 && x.comparators() == 3 && x.min_period_fs == 1000).unwrap_or(false), "三量齐");
    // 族十：页保护透传。
    cs.add("保护 0", p.as_ref().map(|x| x.prot == 0).unwrap_or(false), "无保护");
    let mut pr = mk_hpet(0, 0xFED0_0000, 2, 10);
    pr[HPET_HDR + OFF_HT_PROT] = 4; // 4 = EFI 免收
    cs.add("保护透传", parse_hpet(&pr).map(|x| x.prot == 4).unwrap_or(false), "字段直通");
    cs
}
