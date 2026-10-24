//! unx0473_rescan —— UNX-F0473 快照复扫 diff（枚举稳定性）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0473 · 快照复扫 diff · 目标 220 行
//! 判据原文：UNX-F0473-J1 双轮枚举 diff 账：首轮快照（设备
//!           键 = bus<<8 | dev<<3 | fn 序列）与复扫快照对
//!           比：账二 稳定核：两次快照完全相等 → 枚举稳定
//!           （热插拔静默期）；账三 diff 分类：新增（复扫
//!           有首轮无）、消失（首轮有复扫无）、移位（键同但
//!           class/vendor 变——设备被替换）；账四 diff 不立
//!           案纪律：热插拔是合法事件，diff 只归档不阻断；
//!           账五 空对空合法（零设备总线两轮皆空 = 稳定）。
//! 组件：键投影器 + 三类 diff 器 + 稳定核 + 归档账。
//! 职责定位：复扫 diff 是枚举的「前后对账单」——热插拔、设
//!   备替换、固件重布都会在两轮快照间留下痕迹；把「变化」
//!   分成三类（新增/消失/移位）才能把事件说清楚，混作一句
//!   「拓扑变了」等于什么都没说。
//! 性能逐项分解：diff O(n²)（n ≤ 256 键）。
//! 诊断码段：0x34680（0x34681~0x34689 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断码段位（u32 高段独占 0x34680 起）。
pub const SEG_BASE: u32 = 0x34680;

/// 设备快照条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DevSnap {
    /// bus<<8 | dev<<3 | fn。
    pub key: u16,
    pub vendor: u16,
    pub class: u8,
}

/// diff 类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiffKind {
    Added,
    Removed,
    Moved,
}

/// diff 归档条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiffRec {
    pub kind: DiffKind,
    pub key: u16,
}

/// 三类 diff 器。
pub fn diff(a: &[DevSnap], b: &[DevSnap]) -> Vec<DiffRec> {
    let mut out: Vec<DiffRec> = Vec::new();
    // 新增：b 有 a 无。
    for y in b {
        if !a.iter().any(|x| x.key == y.key) {
            out.push(DiffRec { kind: DiffKind::Added, key: y.key });
        }
    }
    // 消失：a 有 b 无。
    for x in a {
        if !b.iter().any(|y| y.key == x.key) {
            out.push(DiffRec { kind: DiffKind::Removed, key: x.key });
        }
    }
    // 移位：键同但身份变。
    for x in a {
        if let Some(y) = b.iter().find(|y| y.key == x.key) {
            if x.vendor != y.vendor || x.class != y.class {
                out.push(DiffRec { kind: DiffKind::Moved, key: x.key });
            }
        }
    }
    out
}

/// 稳定核：diff 空 = 稳定。
pub fn stable(a: &[DevSnap], b: &[DevSnap]) -> bool {
    diff(a, b).is_empty()
}

/// 判据入口：UNX-F0473 全族核。
pub fn run_unx0473_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0473");

    fn snap(key: u16, vendor: u16, class: u8) -> DevSnap {
        DevSnap { key, vendor, class }
    }

    // 族一：完全稳定。
    let s1 = vec![snap(0x0000, 0x8086, 0x06), snap(0x0100, 0x10DE, 0x03)];
    cs.add("双轮全等稳定", stable(&s1, &s1), "diff 空");
    cs.add("自 diff 空", diff(&s1, &s1).is_empty(), "恒等");

    // 族二：新增。
    let s2 = vec![snap(0x0000, 0x8086, 0x06), snap(0x0200, 0x8086, 0x02)];
    let d2 = diff(&s1, &s2);
    cs.add("新增一行", d2.len() == 1, "0x0200");
    cs.add("新增类别", d2[0].kind == DiffKind::Added && d2[0].key == 0x0200, "键对");

    // 族三：消失。
    let d3 = diff(&s2, &s1);
    cs.add("消失一行", d3.len() == 1, "0x0200");
    cs.add("消失类别", d3[0].kind == DiffKind::Removed, "Removed");

    // 族四：移位（设备替换）。
    let s4 = vec![snap(0x0000, 0x8086, 0x06), snap(0x0100, 0x1002, 0x03)]; // vendor 变
    let d4 = diff(&s1, &s4);
    cs.add("移位一行", d4.len() == 1, "0x0100");
    cs.add("移位类别", d4[0].kind == DiffKind::Moved, "身份变");

    // 族五：class 变也算移位。
    let s5 = vec![snap(0x0000, 0x8086, 0x06), snap(0x0100, 0x10DE, 0x02)];
    let d5 = diff(&s1, &s5);
    cs.add("class 变移位", d5[0].kind == DiffKind::Moved, "同 vendor 异 class");

    // 族六：混合 diff。
    let s6 = vec![snap(0x0000, 0x8086, 0x06), snap(0x0300, 0x1234, 0x01), snap(0x0100, 0xABCD, 0x03)];
    let d6 = diff(&s1, &s6);
    cs.add("混合三行", d6.len() == 3, "新+消+移");
    cs.add("混合含三类", d6.iter().any(|r| r.kind == DiffKind::Added)
        && d6.iter().any(|r| r.kind == DiffKind::Removed)
        && d6.iter().any(|r| r.kind == DiffKind::Moved), "类别齐");

    // 族七：空对空。
    cs.add("空对空稳定", stable(&[], &[]), "零设备");
    let d7 = diff(&s1, &[]);
    cs.add("全消失", d7.len() == 2 && d7.iter().all(|r| r.kind == DiffKind::Removed), "拔光");

    // 族八：键投影语义。
    let k = (0x02u16 << 8) | (0x1Cu16 << 3) | 0x3;
    cs.add("键位域", k == 0x0263, "bus<<8|dev<<3|fn");

    cs
}
