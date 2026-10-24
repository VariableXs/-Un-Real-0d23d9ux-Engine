//! unx0471_xchk —— UNX-F0471 ECAM/CF8 交叉验证（全量）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0471 · ECAM/CF8 交叉验证 · 目标 230 行
//! 判据原文：UNX-F0471-J1 双通道地址换算一致性：同一
//!           (bus, dev, fn) 在 ECAM 寻址（bus<<20 | dev<<15
//!           | fn<<12）与 CF8 遗留寻址（0x80000000 | bus<<16
//!           | dev<<11 | fn<<8）两换算下，读出的头 16 字节
//!           必须逐字节相等（模型面：两视图同源投影）；账二
//!           换算守恒：ECAM 偏移 = CF8 目标寄存器偏移（低
//!           12 位对齐相同寄存器）；账三 交叉覆盖：全量设备
//!           逐一交叉（批量核对账）；账四 失配立案：任一字
//!           节不等 → 立案（寻址 bug 或镜像撕裂）；账五 段
//!           维度：ECAM 支持段号（> 0 段 CF8 不可达 → 交叉
//!           跳过合法，不立案）。
//! 组件：双换算器 + 逐字节对冲器 + 批量交叉账 + 段闸。
//! 职责定位：交叉验证是两套寻址的「对镜自查」——ECAM 是新
//!   世界（MMIO 大窗），CF8 是老世界（端口 0xCF8/0xCFC）；
//!   两镜照出不同的脸，说明有一面镜子裂了（换算 bug 或配
//!   置镜像不一致），在依赖任何一方之前必须先对镜。
//! 性能逐项分解：换算 O(1)；对冲 O(16)；批量 O(n·16)。
//! 诊断码段：0x34600（0x34601~0x34609 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use crate::checks::CheckSet;

/// 诊断码段位（u32 高段独占 0x34600 起）。
pub const SEG_BASE: u32 = 0x34600;
pub const XC_MISMATCH: u32 = SEG_BASE + 1; // 失配
pub const XC_ALIGN16: u32 = SEG_BASE + 2; // 对齐非 4 字节寄存器

/// ECAM 换算（段 0 简化；完整含段 <<28 域）。
pub fn ecam_addr(seg: u16, bus: u8, dev: u8, fun: u8, reg: u8) -> u64 {
    ((seg as u64) << 28)
        | ((bus as u64) << 20)
        | ((dev as u64) << 15)
        | ((fun as u64) << 12)
        | ((reg as u64) & 0xFC)
}

/// CF8 换算（段 0 限定；寄存器低 2 位须零）。
pub fn cf8_addr(bus: u8, dev: u8, fun: u8, reg: u8) -> Result<u32, u32> {
    if reg & 0x03 != 0 {
        return Err(XC_ALIGN16);
    }
    Ok(0x8000_0000
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((fun as u32) << 8)
        | (reg as u32 & 0xFC))
}

/// 段维度闸：段 > 0 → CF8 不可达（交叉跳过合法）。
pub fn cf8_reachable(seg: u16) -> bool {
    seg == 0
}

/// 16 字节头对冲（模型面：两视图由同源镜像投影——此处模拟
/// 两个独立读出口，真实实现为 ECAM MMIO 与 CF8 端口双读）。
pub fn cross_check(
    read_ecam: &[u8; 16],
    read_cf8: &[u8; 16],
) -> Result<(), u32> {
    for i in 0..16 {
        if read_ecam[i] != read_cf8[i] {
            return Err(XC_MISMATCH);
        }
    }
    Ok(())
}

/// 批量交叉账：设备清单逐一交叉，失配计数与全绿判定。
pub fn cross_batch(devs: &[([u8; 16], [u8; 16])]) -> (usize, usize, bool) {
    let total = devs.len();
    let mut ok = 0usize;
    for (e, c) in devs {
        if cross_check(e, c).is_ok() {
            ok += 1;
        }
    }
    (total, ok, ok == total)
}

/// 换算守恒核：两换算指向同一寄存器（低 12/8 位域同源）。
pub fn reg_consistent(bus: u8, dev: u8, fun: u8, reg: u8) -> bool {
    let e = ecam_addr(0, bus, dev, fun, reg) & 0xFFF;
    match cf8_addr(bus, dev, fun, reg) {
        Ok(a) => (a & 0xFC) as u64 == e,
        Err(_) => false,
    }
}

/// 判据入口：UNX-F0471 全族核。
pub fn run_unx0471_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0471");

    // 族一：ECAM 换算。
    let e = ecam_addr(0, 0x02, 0x1C, 0x3, 0x10);
    cs.add("ECAM 总线域", (e >> 20) & 0xFF == 0x02, "bus<<20");
    cs.add("ECAM 设备域", (e >> 15) & 0x1F == 0x1C, "dev<<15");
    cs.add("ECAM 功能域", (e >> 12) & 0x07 == 0x3, "fn<<12");
    cs.add("ECAM 寄存器域", e & 0xFFC == 0x10, "reg");

    // 族二：CF8 换算。
    let c = cf8_addr(0x02, 0x1C, 0x3, 0x10);
    cs.add("CF8 使能位", c.map(|a| a & 0x8000_0000 != 0).unwrap_or(false), "位 31");
    cs.add("CF8 总线域", c.map(|a| (a >> 16) & 0xFF) == Ok(0x02), "bus<<16");
    cs.add("CF8 设备域", c.map(|a| (a >> 11) & 0x1F) == Ok(0x1C), "dev<<11");
    cs.add("CF8 对齐拒", cf8_addr(0, 0, 0, 0x02) == Err(XC_ALIGN16), "reg 低 2 位");

    // 族三：换算守恒。
    cs.add("寄存器域同源", reg_consistent(0x02, 0x1C, 0x3, 0x10), "低域等");
    cs.add("零号设备同源", reg_consistent(0, 0, 0, 0x00), "根复合体");

    // 族四：段闸。
    cs.add("段 0 可达", cf8_reachable(0), "CF8 限段 0");
    cs.add("段 1 跳过", !cf8_reachable(1), "不可达合法");

    // 族五：逐字节对冲。
    let hdr: [u8; 16] = [0x86, 0x80, 0x34, 0x12, 7, 0, 0, 2, 0, 0, 0, 6, 0, 0, 0x01, 0];
    let (t, ok, all) = cross_batch(&[(hdr, hdr)]);
    cs.add("单设备交叉绿", t == 1 && ok == 1 && all, "镜同");

    // 族六：失配立案。
    let mut bad = hdr;
    bad[5] ^= 0xFF;
    let (t2, ok2, all2) = cross_batch(&[(hdr, bad)]);
    cs.add("失配计数", t2 == 1 && ok2 == 0 && !all2, "差一字即破");
    cs.add("失配码", cross_check(&hdr, &bad) == Err(XC_MISMATCH), "立案");

    // 族七：批量混合。
    let devs = vec![(hdr, hdr), (hdr, bad), (hdr, hdr)];
    let (t3, ok3, all3) = cross_batch(&devs);
    cs.add("批量三二过", t3 == 3 && ok3 == 2, "失配一");
    cs.add("批量不全绿", !all3, "全绿破");

    // 族八：ECAM 段位域（段 1 地址含段域）。
    let e1 = ecam_addr(1, 0, 0, 0, 0);
    cs.add("段 1 域位", (e1 >> 28) == 1, "seg<<28");
    cs.add("段 0 干净", ecam_addr(0, 0, 0, 0, 0) >> 28 == 0, "零段");

    cs
}
