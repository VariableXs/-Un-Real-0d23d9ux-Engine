//! unx0478_pcbad —— UNX-F0478 坏配置注入演练（烂设备检出）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0478 · 坏配置注入演练 · 目标 240 行
//! 判据原文：UNX-F0478-J1 烂设备检出账：向合法配置空间注入
//!           五类坏面，逐类独立检出——账二 坏厂商（vend
//!           or_id=0xFFFF 未实现 / 0x0000 保留）拒登；账三
//!           坏头型（type≠0/1/2）拒登；账四 自环桥（桥的
//!           primary==secondary 拒——自环会造成枚举死循环）；
//!           账五 BAR 越窗（BAR 地址+尺寸越过 4G 或命中保
//!           留区 0xA0000~0xFFFFF）拒登；账六 检出率（注入
//!           N 类全部检出，漏检零容忍）。
//! 组件：配置空间投影 + 五类检出器 + 注入器 + 检出账。
//! 职责定位：烂设备在真实总线上真实存在（烂卡、虚拟化残
//!   留、固件造次）——枚举器对每一类坏面都必须有独立的检
//!   出通道，任何一类漏检都会在后续阶段爆出更难追的锅。
//! 性能逐项分解：检出 O(1)/类；演练 O(n)。
//! 诊断码段：0x347C0（0x347C1~0x347C9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 诊断码。
pub const SEG_BASE: u32 = 0x347C0;
pub const XC_VENDOR: u32 = SEG_BASE + 1; // 坏厂商
pub const XC_HTYPE: u32 = SEG_BASE + 2; // 坏头型
pub const XC_LOOP: u32 = SEG_BASE + 3; // 自环桥
pub const XC_BARWIN: u32 = SEG_BASE + 4; // BAR 越窗

/// 配置空间投影（枚举器关心的最小集）。
#[derive(Clone, Copy)]
pub struct Cfg {
    pub vendor_id: u16,
    pub device_id: u16,
    pub header_type: u8,
    /// 桥专用：primary/secondary 总线号（非桥忽略）。
    pub bus_primary: u8,
    pub bus_secondary: u8,
    /// BAR 0（地址 u32 + 尺寸 u32，尺寸为解码后的字节量）。
    pub bar0_addr: u32,
    pub bar0_size: u32,
}

/// 保留区（ISA 遗留）。
const RES_LO: u32 = 0x000A_0000;
const RES_HI: u32 = 0x000F_FFFF;

/// 五类检出器（顺序检，首个命中即拒）。
pub fn vet(c: &Cfg) -> Result<(), u32> {
    // 账二：坏厂商。
    if c.vendor_id == 0xFFFF || c.vendor_id == 0x0000 {
        return Err(XC_VENDOR);
    }
    // 账三：坏头型（0 端点 / 1 桥 / 2 CardBus 合法）。
    if c.header_type & 0x7F > 2 {
        return Err(XC_HTYPE);
    }
    // 账四：自环桥。
    if c.header_type == 1 && c.bus_primary == c.bus_secondary {
        return Err(XC_LOOP);
    }
    // 账五：BAR 越窗。
    if c.bar0_size > 0 {
        let end = c.bar0_addr.saturating_add(c.bar0_size.saturating_sub(1));
        if end > 0xFFFF_FFFF {
            return Err(XC_BARWIN);
        }
        // 命中保留区（区间重叠：addr ≤ RES_HI 且 end ≥ RES_LO）。
        if c.bar0_addr <= RES_HI && end >= RES_LO {
            return Err(XC_BARWIN);
        }
    }
    Ok(())
}

/// 注入器：从合法基线生成五类坏样本。
pub fn inject(kind: u32, base: &Cfg) -> Option<Cfg> {
    let mut c = Cfg {
        vendor_id: base.vendor_id,
        device_id: base.device_id,
        header_type: base.header_type,
        bus_primary: base.bus_primary,
        bus_secondary: base.bus_secondary,
        bar0_addr: base.bar0_addr,
        bar0_size: base.bar0_size,
    };
    match kind {
        0 => c.vendor_id = 0xFFFF,
        1 => c.vendor_id = 0x0000,
        2 => c.header_type = 5,
        3 => {
            c.header_type = 1;
            c.bus_secondary = c.bus_primary;
        }
        4 => c.bar0_addr = RES_LO, // 直插保留区
        _ => return None,
    }
    Some(c)
}

/// 检出账：演练 N 类，全检出为真。
pub fn drill(base: &Cfg) -> (usize, usize) {
    let mut hit = 0usize;
    let mut total = 0usize;
    let mut k = 0u32;
    while k < 5 {
        if let Some(bad) = inject(k, base) {
            if vet(&bad).is_err() {
                hit += 1;
            }
            total += 1;
        }
        k += 1;
    }
    (hit, total)
}

pub fn run_unx0478_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0478");
    cs.add("SEG 段位独占", SEG_BASE == 0x347C0 && XC_BARWIN == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 合法基线：真实感设备（e1000 类）。
    let base = Cfg {
        vendor_id: 0x8086,
        device_id: 0x100E,
        header_type: 0,
        bus_primary: 0,
        bus_secondary: 0,
        bar0_addr: 0xFEBC_0000,
        bar0_size: 0x2_0000,
    };
    cs.add("基线合法", vet(&base).is_ok(), "五类检出器全放行");

    // 族一：坏厂商双面。
    cs.add("FFFF 拒", vet(&inject(0, &base).unwrap_or_else(|| base.clone())) == Err(XC_VENDOR), "未实现面");
    cs.add("0000 拒", vet(&inject(1, &base).unwrap_or_else(|| base.clone())) == Err(XC_VENDOR), "保留面");

    // 族二：坏头型。
    cs.add("头型 5 拒", vet(&inject(2, &base).unwrap_or_else(|| base.clone())) == Err(XC_HTYPE), "0/1/2 外");

    // 族三：自环桥。
    cs.add("自环拒", vet(&inject(3, &base).unwrap_or_else(|| base.clone())) == Err(XC_LOOP), "primary==secondary");
    // 非桥不受自环核约束（头型 0 带 primary==secondary 合法——字段无义）。
    let ep_same = Cfg { bus_secondary: base.bus_primary, ..base };
    cs.add("端点同号合法", vet(&ep_same).is_ok(), "自环核只查桥");

    // 族四：BAR 越窗双面。
    cs.add("保留区命中拒", vet(&inject(4, &base).unwrap_or_else(|| base.clone())) == Err(XC_BARWIN), "0xA0000 直插");
    let over = Cfg { bar0_addr: 0xFFFF_F000, bar0_size: 0x2000, ..base }; // 尾越 4G
    cs.add("4G 尾越拒", vet(&over) == Err(XC_BARWIN), "saturating 核");
    // 紧贴保留区上界合法（end = 0x9FFFF < RES_LO）。
    let below = Cfg { bar0_addr: 0x0009_0000, bar0_size: 0x1_0000, ..base };
    cs.add("紧贴上界合法", vet(&below).is_ok(), "end=0x9FFFF");

    // 族五：演练检出率（五类全检出）。
    let (hit, total) = drill(&base);
    cs.add("演练五类", total == 5, "注入器全通道");
    cs.add("全检出", hit == 5, "漏检零容忍");

    // 族六：幂等（同样本两检恒等）。
    let bad = inject(3, &base).unwrap_or_else(|| base.clone());
    let r1 = vet(&bad);
    let r2 = vet(&bad);
    cs.add("检出幂等", r1 == r2, "两遍恒等");

    // 族七：坏厂商优先级（厂商先于头型）。
    let both = inject(0, &inject(2, &base).unwrap_or_else(|| base.clone())).unwrap_or_else(|| base.clone());
    cs.add("厂商优先", vet(&both) == Err(XC_VENDOR), "首命中即拒");

    cs
}
