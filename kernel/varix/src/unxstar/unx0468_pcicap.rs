//! unx0468_pcicap —— UNX-F0468 能力链表账（MSI/MSI-X/PM 只读）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0468 · 能力链表账 · 目标 230 行
//! 判据原文：UNX-F0468-J1 能力链表解析账（状态寄存器位 4 =
//!           能力表存在，指针 @0x34）：链表节点 = [cap_id 1B
//!           | next 1B | 载荷...]，next = 0 链尾；账二 三类
//!           识别：ID 0x01 = PM 电源管理、0x05 = MSI（32/64
//!           位按载荷位 7）、0x11 = MSI-X；账三 环检测：遍历
//!           访问集去重，重访 = 链环立案（固件坏链）；账四
//!           边界核：节点偏移必须 4 字节对齐且 ≤ 0xFC（标准
//!           头 256 字节内）；账五 只读纪律：解析无写通道；
//!           未知 ID 跳过不立案（前瞻兼容）。
//! 组件：链表步进器 + 三类识别器 + 环检测账 + 对齐/边界闸。
//! 职责定位：能力链表是 PCI 的「插件声明页」——MSI 中断、
//!   电源管理全挂在链上；固件坏链（环）会让朴素遍历死循环，
//!   环检测账就是遍历器的安全带；MSI 32/64 位能力位读错会
//!   把消息地址截成两半，中断打到虚空。
//! 性能逐项分解：遍历 O(n)（n ≤ 48 槽）；环检测 O(n)。
//! 诊断码段：0x34540（0x34541~0x34549 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断码段位（u32 高段独占 0x34540 起）。
pub const SEG_BASE: u32 = 0x34540;
pub const XC_LOOP: u32 = SEG_BASE + 1; // 链环
pub const XC_ALIGN: u32 = SEG_BASE + 2; // 节点偏移非对齐
pub const XC_OOB: u32 = SEG_BASE + 3; // 越界（> 0xFC）

/// 标准头边界。
pub const HDR_END: usize = 0x100;
/// 能力指针偏移。
pub const CAP_PTR: usize = 0x34;

/// 识别出的能力条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cap {
    pub id: u8,
    pub offset: u8,
    /// MSI 64 位能力（ID 0x05 载荷位 7）。
    pub msi64: bool,
}

/// 能力类别投影。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CapKind {
    Pm,
    Msi,
    MsiX,
    Unknown,
}

pub fn kind_of(id: u8) -> CapKind {
    match id {
        0x01 => CapKind::Pm,
        0x05 => CapKind::Msi,
        0x11 => CapKind::MsiX,
        _ => CapKind::Unknown,
    }
}

/// 链表解析：cfg 为 256 字节配置空间，has_cap 为状态寄存器
/// 位 4。返回能力条目集；环/越界/非对齐立案。
pub fn walk_caps(cfg: &[u8], has_cap: bool) -> Result<Vec<Cap>, u32> {
    let mut out: Vec<Cap> = Vec::new();
    if !has_cap {
        return Ok(out);
    }
    if cfg.len() < HDR_END {
        return Err(XC_OOB);
    }
    let mut off = cfg[CAP_PTR] as usize;
    let mut seen: Vec<usize> = Vec::new();
    while off != 0 {
        // 环检测：重访立案。
        if seen.contains(&off) {
            return Err(XC_LOOP);
        }
        seen.push(off);
        // 边界 + 对齐核。
        if off > 0xFC {
            return Err(XC_OOB);
        }
        if off & 0x3 != 0 {
            return Err(XC_ALIGN);
        }
        if off + 2 > cfg.len() {
            return Err(XC_OOB);
        }
        let id = cfg[off];
        let next = cfg[off + 1] as usize;
        // MSI 64 位位（载荷第二字节位 7——MSI 控制寄存器 @+2）。
        let msi64 = id == 0x05 && off + 3 < cfg.len() && cfg[off + 3] & 0x80 != 0;
        out.push(Cap { id, offset: off as u8, msi64 });
        off = next;
    }
    Ok(out)
}

/// 判据入口：UNX-F0468 全族核。
pub fn run_unx0468_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0468");

    fn fixture(caps: &[u8]) -> ([u8; 256], bool) {
        let mut c = [0u8; 256];
        // 链：caps = [(id, payload_len)...]，写在 0x40、0x50、0x60。
        let mut prev_next = 0u8;
        for (i, &id) in caps.iter().enumerate().rev() {
            let off = 0x40 + i * 0x10;
            c[off] = id;
            c[off + 1] = prev_next;
            prev_next = off as u8;
        }
        c[CAP_PTR] = prev_next;
        let has = !caps.is_empty();
        (c, has)
    }

    // 族一：无能力表。
    let (c0, _h0) = fixture(&[]);
    cs.add("无旗空表", walk_caps(&c0, false).is_ok() && walk_caps(&c0, false).unwrap_or_default().is_empty(), "位 4 清");

    // 族二：三类识别。
    let (c3, h3) = fixture(&[0x01, 0x05, 0x11]);
    let caps3 = walk_caps(&c3, h3).unwrap_or_default();
    cs.add("三节点", caps3.len() == 3, "PM+MSI+MSI-X");
    cs.add("PM 识别", caps3.iter().any(|c| kind_of(c.id) == CapKind::Pm), "0x01");
    cs.add("MSI 识别", caps3.iter().any(|c| kind_of(c.id) == CapKind::Msi), "0x05");
    cs.add("MSI-X 识别", caps3.iter().any(|c| kind_of(c.id) == CapKind::MsiX), "0x11");

    // 族三：链序（从指针起）。
    cs.add("链首 PM", caps3.first().map(|c| c.id) == Some(0x01), "指针指向");
    cs.add("偏移记录", caps3.first().map(|c| c.offset) == Some(0x40), "0x40");

    // 族四：MSI 64 位位。
    let mut c64 = c3;
    c64[0x40 + 3] = 0x80; // PM 位（非 MSI）——不误判
    let capsA = walk_caps(&c64, true).unwrap_or_default();
    cs.add("非 MSI 无 64 位", capsA.iter().all(|c| !c.msi64 || c.id == 0x05), "位域隔离");
    // 真 MSI 64 位：MSI 节点 @0x50，控制字节 @0x53。
    c64[0x50 + 3] = 0x80;
    let capsB = walk_caps(&c64, true).unwrap_or_default();
    cs.add("MSI 64 位真", capsB.iter().any(|c| c.id == 0x05 && c.msi64), "位 7");

    // 族五：环检测。
    let mut cloop = c3;
    cloop[0x60 + 1] = 0x40; // 尾指回首 → 环
    cs.add("链环立案", walk_caps(&cloop, true) == Err(XC_LOOP), "重访");

    // 族六：对齐核。
    let mut calign = c3;
    calign[CAP_PTR] = 0x41; // 非对齐指针
    cs.add("非对齐立案", walk_caps(&calign, true) == Err(XC_ALIGN), "低 2 位");

    // 族七：越界核。
    let mut coob = c3;
    coob[CAP_PTR] = 0xFD; // > 0xFC
    cs.add("越界立案", walk_caps(&coob, true) == Err(XC_OOB), "0xFD");

    // 族八：未知 ID 跳过。
    let (cu, hu) = fixture(&[0x01, 0x42, 0x11]);
    let capsU = walk_caps(&cu, hu).unwrap_or_default();
    cs.add("未知 ID 不立案", walk_caps(&cu, hu).is_ok(), "前瞻");
    cs.add("未知仍入列", capsU.len() == 3, "记 Unknown");
    cs.add("未知类别", capsU.iter().any(|c| kind_of(c.id) == CapKind::Unknown), "0x42");

    // 族九：单节点链尾零。
    let (c1, h1) = fixture(&[0x01]);
    let caps1 = walk_caps(&c1, h1).unwrap_or_default();
    cs.add("单节点", caps1.len() == 1, "next=0 终");

    cs
}
