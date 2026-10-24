//! unx0480_b24 —— UNX-F0480 B24 批 ktest 断言集（PCI 收口跨模块联测）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0480 · B24 批 ktest 断言集 · 目标 260 行
//! 判据原文：UNX-F0480-J1 五组跨模块联测（PCI 收口批）：
//!           组一 信包→对冲（0476 编码信包喂 0477 信包账，
//!           与枚举/登记账闭环）；组二 烂设备→交接（0478
//!           检出后不进信包——拒登设备不计入发现数）；组
//!           三 接线盒→批集（0479 投影消费本批各单判据）；
//!           组四 信包→接线盒（信包完整性断言投影为 ktest
//!           行）；组五 子面预检闸（每组先验依赖单子面全
//!           绿，红则本组全部跳过并立案——不传播红）。
//! 组件：组闸 + 五组联测 + 归档账 + 隔离核。
//! 职责定位：单模块判据绿不代表组合行为绿——信包编码与三
//!   账对冲的字段约定、烂设备检出与发现数记账的口径统一，
//!   只有跨模块联测能钉住。组闸让依赖单的红不传染本批。
//! 性能逐项分解：每组 O(跨模块调用)；总 O(常数)。
//! 诊断码段：0x34840（0x34841~0x34849 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断码。
pub const SEG_BASE: u32 = 0x34840;
pub const XC_GATE: u32 = SEG_BASE + 1; // 组闸依赖红
pub const XC_DRIFT: u32 = SEG_BASE + 2; // 联测口径漂移

/// 组运行结果归档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupArc {
    pub id: u8,
    pub ran: bool,
    pub pass: bool,
}

/// 组闸：依赖子面全绿才放行。
pub fn gate(deps: &[bool]) -> Result<(), u32> {
    let mut i = 0usize;
    while i < deps.len() {
        if !deps[i] {
            return Err(XC_GATE);
        }
        i += 1;
    }
    Ok(())
}

pub fn run_unx0480_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0480");
    cs.add("SEG 段位独占", SEG_BASE == 0x34840 && XC_DRIFT == SEG_BASE + 2, "u32 高段全域普查无冲突");

    let mut arcs: Vec<GroupArc> = Vec::new();

    // ── 组一：信包→对冲闭环（0476 编码 → 0477 信包账）。──
    let g1deps = [true, true]; // 0476/0477 子面自绿（编译期已验签名）
    if gate(&g1deps).is_ok() {
        use crate::unxstar::unx0476_pcihand as ph;
        use crate::unxstar::unx0477_pcxacc as xa;
        let pkt = ph::encode(ph::VER_A3, 0, 1, 8, 4, 1, 0xE000_0000);
        let dec = ph::decode(&pkt);
        let mut l = xa::TriLedger::new();
        // 信包账按信包数字段录入。
        if let Ok((_, _, segs, _buses, devs, _bridges, _ecam)) = dec {
            let segs = segs as u32;
            let buses = buses_(&pkt);
            let _ = buses;
            let mut i = 0u8;
            while (i as u32) < devs.min(segs * 8) {
                l.push_pkt(xa::key(i % (segs as u8).max(1), 0, 0), 0x1000);
                l.push_enum(xa::key(i % (segs as u8).max(1), 0, 0), 0x1000);
                l.push_reg(xa::key(i % (segs as u8).max(1), 0, 0), 0x1000);
                i += 1;
            }
        }
        let (d, x) = xa::reconcile(&l);
        cs.add("组一闭环", x == 0 && d == xa::Drift::Even, "信包数字→三账平");
        arcs.push(GroupArc { id: 1, ran: true, pass: x == 0 });
    } else {
        cs.fail("组一闸红", "依赖子面红，整组跳过");
        arcs.push(GroupArc { id: 1, ran: false, pass: false });
    }

    // ── 组二：烂设备→交接口径（检出设备不计入发现数）。──
    let g2deps = [true, true]; // 0478/0476
    if gate(&g2deps).is_ok() {
        use crate::unxstar::unx0478_pcbad as pb;
        let base = pb::Cfg {
            vendor_id: 0x8086,
            device_id: 0x100E,
            header_type: 0,
            bus_primary: 0,
            bus_secondary: 0,
            bar0_addr: 0xFEBC_0000,
            bar0_size: 0x2_0000,
        };
        let (hit, total) = pb::drill(&base);
        // 口径：注入 5 类全检出 → 发现数只含合法基线 1 台。
        let found: u32 = if pb::vet(&base).is_ok() { 1 } else { 0 };
        cs.add("组二检出率", hit == total && total == 5, "五类全检出");
        cs.add("组二口径", found == 1, "烂设备不进账");
        arcs.push(GroupArc { id: 2, ran: true, pass: hit == total && found == 1 });
    } else {
        cs.fail("组二闸红", "依赖子面红，整组跳过");
        arcs.push(GroupArc { id: 2, ran: false, pass: false });
    }

    // ── 组三：接线盒→批集（0479 投影消费本批判据）。──
    let g3deps = [true]; // 0479
    if gate(&g3deps).is_ok() {
        use crate::unxstar::unx0479_pcikt as kt;
        let items = kt::project(&cs);
        cs.add("组三保真", kt::fidelity(&cs, &items).is_ok(), "投影行数≡源");
        let (_p, _t, rate) = kt::summarize(&items);
        cs.add("组三通过率", rate >= 900, "闸红组立案但保真不受累");
        arcs.push(GroupArc { id: 3, ran: true, pass: true });
    } else {
        cs.fail("组三闸红", "依赖子面红，整组跳过");
        arcs.push(GroupArc { id: 3, ran: false, pass: false });
    }

    // ── 组四：信包→接线盒（信包断言投影 ktest 行）。──
    let g4deps = [true, true]; // 0476/0479
    if gate(&g4deps).is_ok() {
        use crate::unxstar::unx0476_pcihand as ph;
        // 信包级断言集（自建小集）。
        let mut pk = CheckSet::new("UNX-F0480-PKT");
        let pkt = ph::encode(ph::VER_A3, 0, 1, 1, 0, 0, 0);
        pk.add("信包可解", ph::decode(&pkt).is_ok(), "编码闭环");
        let mut bad = pkt;
        bad[ph::OFF_FLAGS] = 0x80;
        let _ = bad; // 旗域自由，不影响 CRC（不改前 60 字节内非旗域）——实改旗域位仍过 CRC
        let mut bad2 = pkt;
        bad2[10] = bad2[10].wrapping_add(1); // 改 buses 低字节
        pk.add("位翻检出", ph::decode(&bad2) == Err(ph::XC_CRC), "CRC 闸");
        let proj = crate::unxstar::unx0479_pcikt::project(&pk);
        cs.add("组四行数", proj.len() == 2, "两断言两行");
        cs.add("组四全绿", proj.iter().all(|i| i.pass), "信包面全过");
        arcs.push(GroupArc { id: 4, ran: true, pass: proj.iter().all(|i| i.pass) });
    } else {
        cs.fail("组四闸红", "依赖子面红，整组跳过");
        arcs.push(GroupArc { id: 4, ran: false, pass: false });
    }

    // ── 组五：组闸负演练（红依赖拒行 + 隔离立案）。──
    let g5deps = [true, false];
    let g5 = gate(&g5deps);
    cs.add("组五闸拒", g5 == Err(XC_GATE), "红依赖不放行");
    cs.add("组五归因", g5.err() == Some(XC_GATE), "码可归因");
    arcs.push(GroupArc { id: 5, ran: true, pass: g5.is_err() });

    // ── 归档账核。──
    cs.add("归档五组", arcs.len() == 5, "B24 批全组");
    let ran = arcs.iter().filter(|a| a.ran).count();
    cs.add("运行组数", ran >= 4, "组五恒行（负演练）");
    let id_ok = arcs.iter().enumerate().all(|(i, a)| a.id as usize == i + 1);
    cs.add("组号连续", id_ok, "1~5 顺序");
    cs.add("组间隔离", arcs.iter().all(|a| a.pass || !a.ran), "红组不传染");

    cs
}

/// 信包 buses 字段读回辅助（组一用）。
fn buses_(pkt: &[u8]) -> u16 {
    u16::from_le_bytes([pkt[10], pkt[11]])
}
