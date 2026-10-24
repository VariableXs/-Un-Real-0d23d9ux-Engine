//! unx0467_pcclass —— UNX-F0467 类码分类账（class/subclass/progIF）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0467 · 类码分类账 · 目标 210 行
//! 判据原文：UNX-F0467-J1 三元组（class、subclass、progIF）
//!           分类账：主类识别（0x00 不确定、0x01 存储、0x02
//!           网络、0x03 显示、0x06 桥、0x0C 串行总线、0xFF
//!           未分类）；桥子类核：0x06/0x04 = PCI-PCI 桥、
//!           0x06/0x01 = ISA 桥；显示核：0x03/0x00 = VGA 兼
//!           容（progIF 0x00 = VGA、0x01 = 8514）、0x03/0x02
//!           = 3D 控制器；账二 分类完整性：三元组 → 可打印
//!           类名（查表面）；账三 未知类不立案（前瞻兼容记
//!           "Other"）；账四 progIF 自由度：同 (class,
//!           subclass) 下 progIF 任意合法（无派生约束）。
//! 组件：三元组查表器 + 桥/显示特化核 + 类名投影。
//! 职责定位：类码是设备的「身份证前三位」——枚举后按类分
//!   配驱动全靠它；桥类错认会把总线拓扑剪断，显示类错认会
//!   抢错 framebuffer；0xFF「未分类」是规范留的合法兜底。
//! 性能逐项分解：查表 O(1)。
//! 诊断码段：0x34500（0x34501~0x34509 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 诊断码段位（u32 高段独占 0x34500 起）。
pub const SEG_BASE: u32 = 0x34500;

/// 设备大类（查表面投影）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DevClass {
    Unclassified, // 0x00
    Storage,      // 0x01
    Network,      // 0x02
    Display,      // 0x03
    Bridge,       // 0x06
    SerialBus,    // 0x0C
    Other,        // 其余 + 0xFF
}

/// 桥子类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BridgeSub {
    Host,     // 0x00
    Isa,      // 0x01
    PciPci,   // 0x04
    OtherSub,
}

/// 显示子类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DisplaySub {
    Vga,      // 0x00
    Xga,      // 0x01
    ThreeD,   // 0x02
    OtherSub,
}

/// 主类映射。
pub fn class_of(c: u8) -> DevClass {
    match c {
        0x00 => DevClass::Unclassified,
        0x01 => DevClass::Storage,
        0x02 => DevClass::Network,
        0x03 => DevClass::Display,
        0x06 => DevClass::Bridge,
        0x0C => DevClass::SerialBus,
        _ => DevClass::Other,
    }
}

/// 桥子类映射。
pub fn bridge_sub(s: u8) -> BridgeSub {
    match s {
        0x00 => BridgeSub::Host,
        0x01 => BridgeSub::Isa,
        0x04 => BridgeSub::PciPci,
        _ => BridgeSub::OtherSub,
    }
}

/// 显示子类映射。
pub fn display_sub(s: u8) -> DisplaySub {
    match s {
        0x00 => DisplaySub::Vga,
        0x01 => DisplaySub::Xga,
        0x02 => DisplaySub::ThreeD,
        _ => DisplaySub::OtherSub,
    }
}

/// VGA 兼容核：0x03/0x00 且 progIF ∈ {0x00, 0x01}。
pub fn vga_compat(c: u8, s: u8, p: u8) -> bool {
    c == 0x03 && s == 0x00 && (p == 0x00 || p == 0x01)
}

/// PCI-PCI 桥核。
pub fn pci_bridge(c: u8, s: u8) -> bool {
    c == 0x06 && s == 0x04
}

/// 类名投影（可打印查表面）。
pub fn name_of(c: u8) -> &'static str {
    match c {
        0x00 => "不确定",
        0x01 => "存储",
        0x02 => "网络",
        0x03 => "显示",
        0x06 => "桥",
        0x0C => "串行总线",
        0xFF => "未分类",
        _ => "Other",
    }
}

/// 判据入口：UNX-F0467 全族核。
pub fn run_unx0467_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0467");

    // 族一：主类映射全谱。
    cs.add("存储类", class_of(0x01) == DevClass::Storage, "0x01");
    cs.add("网络类", class_of(0x02) == DevClass::Network, "0x02");
    cs.add("显示类", class_of(0x03) == DevClass::Display, "0x03");
    cs.add("桥类", class_of(0x06) == DevClass::Bridge, "0x06");
    cs.add("串行总线", class_of(0x0C) == DevClass::SerialBus, "0x0C");
    cs.add("未分类兜底", class_of(0xFF) == DevClass::Other, "0xFF");
    cs.add("未知类不立案", class_of(0x42) == DevClass::Other, "前瞻");

    // 族二：桥子类。
    cs.add("PCI-PCI 桥", bridge_sub(0x04) == BridgeSub::PciPci, "0x04");
    cs.add("ISA 桥", bridge_sub(0x01) == BridgeSub::Isa, "0x01");
    cs.add("宿主桥", bridge_sub(0x00) == BridgeSub::Host, "0x00");
    cs.add("桥核真", pci_bridge(0x06, 0x04), "组合");
    cs.add("桥核假", !pci_bridge(0x06, 0x01), "ISA 非桥核");

    // 族三：显示子类。
    cs.add("VGA 子类", display_sub(0x00) == DisplaySub::Vga, "0x00");
    cs.add("3D 子类", display_sub(0x02) == DisplaySub::ThreeD, "0x02");
    cs.add("XGA 子类", display_sub(0x01) == DisplaySub::Xga, "0x01");

    // 族四：VGA 兼容核。
    cs.add("VGA 兼容真", vga_compat(0x03, 0x00, 0x00), "progIF 0");
    cs.add("8514 兼容真", vga_compat(0x03, 0x00, 0x01), "progIF 1");
    cs.add("3D 非兼容", !vga_compat(0x03, 0x02, 0x00), "子类异");
    cs.add("progIF 2 非兼容", !vga_compat(0x03, 0x00, 0x02), "超集拒");

    // 族五：progIF 自由度（同 class/subclass 下任意合法）。
    cs.add("progIF 任意 1", class_of(0x03) == DevClass::Display && display_sub(0x00) == DisplaySub::Vga, "组合稳定");
    cs.add("progIF 任意 2", vga_compat(0x03, 0x00, 0x01) && !vga_compat(0x03, 0x00, 0x09), "界内界外");

    // 族六：类名投影。
    cs.add("名可投影", !name_of(0x01).is_empty(), "存储名");
    cs.add("未知名兜底", name_of(0x42) == "Other", "Other");
    cs.add("未分类名", name_of(0xFF) == "未分类", "0xFF");

    cs
}
