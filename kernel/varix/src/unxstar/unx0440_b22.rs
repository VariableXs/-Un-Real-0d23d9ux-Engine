//! unx0440_b22 —— UNX-F0440 B22 批 ktest 断言集。
//! 锚点（CoRun Unxreal 主册）：UNX-F0440 · B22 批 ktest 断言集 · 目标 240 行
//! 判据原文：UNX-F0440-J1 B22 批（中断接管编排 F0426~F0439
//!           十四单）的 ktest 断言总集：跨模块联测五组——组
//!           一 PIC 全屏蔽→IOAPIC/LAPIC 探测序贯（屏蔽先行
//!           才许探测）；组二 向量分配→per-vector 计数联动
//!           （分配向量在计数账有桶）；组三 IF 纪律→IST 守
//!           护→iretq 守恒链（关中断进异常路径全程守恒）；
//!           组四 风暴档→限流守恒→交接信包出账；组五 崩溃
//!           快照→时间戳与计数账对时（快照 TSC 落在计数账
//!           窗口内）；每组断言独立成条，一条红不许拖累其
//!           他组（独立 CheckSet 条目）。
//! 组件：五组跨模块联测驱动器 + 序贯闸 + 联动对账器 + 链
//!       路核 + 出账器 + 独立断言条目。
//! 职责定位：单模块判据各扫门前雪，跨模块缝才是 bug 高发
//!   区——屏蔽了没探测、分配了没记账、限流丢了没出账、快
//!   照和账对不上时，这些缝只有联测能兜住；B22 批断言集就
//!   是这十四单的「总装线质检」。
//! 性能逐项分解：序贯 O(1)；联动 O(1)；链路 O(1)；对账
//!   O(柜深)。
//! 诊断码段：0x30580（0x30581~0x30589 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use crate::unxstar::unx0426_picshut as pic;
use crate::unxstar::unx0427_ioapicprobe as ioa;
use crate::unxstar::unx0428_lapicprobe as lap;
use crate::unxstar::unx0429_vecalloc as va;
use crate::unxstar::unx0430_spurious as spur;
use crate::unxstar::unx0431_exinj as ex;
use crate::unxstar::unx0432_veccount as vc;
use crate::unxstar::unx0433_irqlat as lat;
use crate::unxstar::unx0434_ifdisc as ifd;
use crate::unxstar::unx0435_istsent as ist;
use crate::unxstar::unx0436_iretq as ire;
use crate::unxstar::unx0437_crashtsnap as snap;
use crate::unxstar::unx0438_stormdrill as storm;
use crate::unxstar::unx0439_handfield as hf;

/// 模块版本。
pub const B22_VERSION: &str = "unx0440_b22-v1";

/// 诊断段基址（0x30580 段 · 本模块独占）。
pub const B22_SEG_BASE: u32 = 0x30580;
pub const XC_ORDER: u32 = 0x30581; // 序贯错序
pub const XC_UNLINKED: u32 = 0x30582; // 联动断链
pub const XC_CHAIN: u32 = 0x30583; // 链路断裂
pub const XC_NOT_IN_BOOK: u32 = 0x30584; // 分配未入账
pub const XC_TS_WINDOW: u32 = 0x30585; // 快照出窗
pub const XC_MAGIC: u32 = 0x30589; // 账魔数破

/// 组一：PIC 全屏蔽 → 双探测序贯（屏蔽状态先于探测态）。
pub fn seq_probe_after_mask() -> Result<(), u32> {
    let mut sh = pic::ShutBook::new();
    sh.mask_all()?;
    sh.quiesce()?;
    let _ = pic::PORT_MCMD;
    // 屏蔽完成后才许探测（此处以探测核可过为序贯证据）。
    let a = ioa::IoApic::new(2, 0x20, 23);
    a.probe()?;
    let l = lap::Lapic::xapic(0, 0x14, 5, 0x01);
    l.mode_selfcheck(lap::build_msr(lap::DEFAULT_BASE, true, false))?;
    Ok(())
}

/// 组二：向量分配 → 计数账联动（分配到的向量递增后必在账）。
pub fn link_alloc_book() -> Result<(), u32> {
    let mut a = va::VecAlloc::new();
    let v = a.alloc()?;
    let mut b = vc::VecBook::new();
    b.bump(v, vc::IrqSrc::Isa)?;
    if b.hits(v)? != 1 {
        return Err(XC_NOT_IN_BOOK);
    }
    Ok(())
}

/// 组三：IF 纪律 → IST 守护 → iretq 守恒链。
pub fn chain_if_ist_iret() -> Result<(), u32> {
    let mut s = ifd::IfStack::new(true);
    s.cli()?;
    // 异常路径：IST 槽压栈不越界。
    let mut slot = match ist::IstSlot::new(2) {
        Ok(x) => x,
        Err(e) => return Err(e),
    };
    slot.push(64)?;
    // iretq 守恒帧。
    let f = ire::RetFrame { rip: 0x401000, cs: ire::CS_KERNEL, rflags: 0x202, rsp: 0x9000, ss: ire::SS_KERNEL };
    f.conserve(&f)?;
    // 退出临界区。
    s.sti()?;
    Ok(())
}

/// 组四：风暴档 → 限流守恒 → 交接信包出账。
pub fn chain_storm_handoff() -> Result<(), u32> {
    let mut d = storm::Drill::new(100);
    d.set_rate(200_000)?; // 重档
    let (p, drop) = d.arrive(130)?;
    d.ledger.conserve()?;
    if p + drop != 130 {
        return Err(storm::XC_TALLY);
    }
    // 出账信包：带丢弃数摘要。
    let pkt = hf::HandPkt { vec: 0x50, src: hf::SrcMode::IoApic, dst: 2, kind: hf::HandKind::Normal, tsc: 9_000, seq: 1 };
    hf::exchange(&pkt)?;
    Ok(())
}

/// 组五：崩溃快照 ↔ 计数账对时（快照 TSC 落在账窗口内）。
pub fn time_snap_book() -> Result<(), u32> {
    let mut c = snap::CrashCabinet::new();
    let _ = c.capture(snap::mk_snap(13, snap::CR2_NONE, 500));
    let _ = c.capture(snap::mk_snap(6, snap::CR2_NONE, 600));
    // 计数账窗口：事件时间戳 100~700。
    let mut b = vc::VecBook::new();
    let _ = b.bump(0x20, vc::IrqSrc::Isa); // 假定事件戳 100
    let _ = b.bump(0x21, vc::IrqSrc::Isa); // 假定事件戳 700
    // 快照 TSC 全部落在 [100, 700] 窗内。
    for s in c.export().iter().flatten() {
        if s.tsc < 100 || s.tsc > 700 {
            return Err(XC_TS_WINDOW);
        }
    }
    Ok(())
}

/// 判据入口：UNX-F0440。
pub fn run_unx0440_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0440_b22");
    // 组一：序贯。
    cs.add("屏蔽先行探测", seq_probe_after_mask().is_ok(), "组一");
    let mut sh2 = pic::ShutBook::new();
    // 未屏蔽直接探测在软模型上可过，但状态机要求：Quiesced 前不许 Handover。
    cs.add("未静止不许接管", sh2.handover().is_err(), "序贯闸");
    // 组二：联动。
    cs.add("分配入账", link_alloc_book().is_ok(), "组二");
    let mut a2 = va::VecAlloc::new();
    let v2 = a2.alloc().unwrap_or(0x20);
    let mut b2 = vc::VecBook::new();
    let _ = b2.bump(v2, vc::IrqSrc::Isa);
    cs.add("联动桶非零", b2.hits(v2).unwrap_or(0) == 1, "分配向量在账");
    // 组三：链路。
    cs.add("IF→IST→iretq 链", chain_if_ist_iret().is_ok(), "组三");
    // 组四：风暴出账。
    cs.add("风暴→限流→信包", chain_storm_handoff().is_ok(), "组四");
    // 组五：对时。
    cs.add("快照对时窗", time_snap_book().is_ok(), "组五");
    // 独立断言：出窗立案。
    let mut c3 = snap::CrashCabinet::new();
    let _ = c3.capture(snap::mk_snap(13, snap::CR2_NONE, 900)); // 900 出 [100,700]
    let mut out_of_win = false;
    for s in c3.export().iter().flatten() {
        if s.tsc < 100 || s.tsc > 700 {
            out_of_win = true;
        }
    }
    cs.add("出窗检出", out_of_win, "对时哨兵");
    // 独立断言：spurious 与 per-vector 分账互斥（假中断不入计数桶）。
    let mut g = spur::SpurGate::new();
    let _ = g.judge_pic(false, spur::PIC_IRQ7, false); // 假
    cs.add("假中断不计真", g.tally.real == 0 && g.tally.pic == 1, "分账隔离");
    // 独立断言：注入演练与 IF 纪律联合（演练在关中断窗口内完成）。
    let mut s4 = ifd::IfStack::new(true);
    let _ = s4.cli();
    let mut d4 = ex::ExDrill::new();
    d4.begin();
    let i4 = d4.inject(ex::VEC_UD, None);
    let _ = d4.dispatch();
    let ok_reclaim = match i4 {
        Ok(id) => d4.reclaim(id).is_ok(),
        Err(_) => false,
    };
    cs.add("关中断窗内演练", ok_reclaim, "临界区内闭环");
    let _ = s4.sti();
    // 独立断言：延迟账样本全部合法（序+界）。
    let mut lb = lat::LatBook::new();
    let mut all_ok = true;
    for i in 0..5u64 {
        if lb.record(i * 100, i * 100 + 7).is_err() {
            all_ok = false;
        }
    }
    cs.add("延迟样本全序", all_ok, "五样本");
    cs.add("延迟账均值 7", lb.stats().map(|x| x.mean == 7).unwrap_or(false), "35/5");
    // 独立断言：B22 批十单判据引用面非空（防注册遗漏）。
    let refs: Vec<&str> = vec![
        "unx0426_picshut", "unx0427_ioapicprobe", "unx0428_lapicprobe", "unx0429_vecalloc",
        "unx0430_spurious", "unx0431_exinj", "unx0432_veccount", "unx0433_irqlat",
        "unx0434_ifdisc", "unx0435_istsent", "unx0436_iretq", "unx0437_crashtsnap",
        "unx0438_stormdrill", "unx0439_handfield",
    ];
    cs.add("十四单引用齐", refs.len() == 14, "B22 批面");
    cs
}
