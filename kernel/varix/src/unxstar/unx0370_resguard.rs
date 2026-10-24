//! unx0370_resguard —— UNX-F0370 保留区越界分配拦截。
//! 锚点（CoRun Unxreal 主册）：UNX-F0370 · 保留区拦截 · 目标 200 行
//! 判据原文：UNX-F0370-J1 保留区（SMBIOS/ACPI/启动参数页）登记
//!           后，任何分配器路径请求保留区内帧一律拦截：拦截
//!           按保留段粒度（整段命中即拦）；部分跨界（请求区间
//!           与保留段相交但起点在外）同样拦；拦截事件入账
//!           （谁在哪一刻想碰保留区——事后审计的关键物证）。
//! 组件：保留段账（升序登记 + 部分跨界核）+ 拦截引擎 + 拦截
//!       事件账（定深环形）。
//! 职责定位：保留区拦截是「固件资产的守夜人」——SMBIOS 表被
//!   分配器覆写不会立刻炸，会在三天后的 DMI 查询时返回垃圾；
//!   「起点合法但尾巴跨界」是最常见的漏网形态，必须按区间
//!   相交判（不是按起点判）。
//! 性能逐项分解：拦截 O(段数)；登记 O(段数)；事件账 O(1)。
//! 诊断码段：0x20900（0x20901~0x20909 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const RG_VERSION: &str = "unx0370_resguard-v1";

/// 诊断段基址（0x20900 段 · 本模块独占）。
pub const RG_SEG_BASE: u32 = 0x20900;
pub const XC_RANGE_BAD: u32 = 0x20901; // 段起止倒挂
pub const XC_OVERLAP: u32 = 0x20902; // 保留段重叠
pub const XC_ORDER_OOO: u32 = 0x20903; // 段表乱序
pub const XC_RESERVE_HIT: u32 = 0x20904; // 拦截命中
pub const XC_EVENT_FULL: u32 = 0x20905; // 事件环满
pub const XC_SEG_EMPTY: u32 = 0x20906; // 空账拒
pub const XC_KIND_OOR: u32 = 0x20907; // 类别越域
pub const XC_CROSS_PARTIAL: u32 = 0x20908; // 部分跨界
pub const XC_HITS_PARTIAL: u32 = 0x20909; // 相交核自身破

/// 保留类别。
pub const K_SMBIOS: u8 = 1;
pub const K_ACPI: u8 = 2;
pub const K_BOOTARG: u8 = 3;
pub const K_RESERVED: u8 = 4;

/// 保留段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReserveSeg {
    pub start: u32,
    pub end: u32, // 开区间端
    pub kind: u8,
}

impl ReserveSeg {
    pub fn new(start: u32, end: u32, kind: u8) -> Result<ReserveSeg, u32> {
        if end <= start {
            return Err(XC_RANGE_BAD);
        }
        if !matches!(kind, K_SMBIOS | K_ACPI | K_BOOTARG | K_RESERVED) {
            return Err(XC_KIND_OOR);
        }
        Ok(ReserveSeg { start, end, kind })
    }
}

/// 保留区守卫。
pub struct ResGuard {
    segs: Vec<ReserveSeg>,
    events: Vec<(u32, u32, u8)>, // (拦截地址, 请求帧数, 命中类别) 定深环
    ev_cap: usize,
    ev_total: u64,
}

impl ResGuard {
    pub fn new(segs: &[ReserveSeg]) -> Result<ResGuard, u32> {
        if segs.is_empty() {
            return Err(XC_SEG_EMPTY);
        }
        for (i, s) in segs.iter().enumerate() {
            if s.end <= s.start {
                return Err(XC_RANGE_BAD);
            }
            if i > 0 {
                if s.start < segs[i - 1].end {
                    return Err(XC_OVERLAP);
                }
                if s.start <= segs[i - 1].start {
                    return Err(XC_ORDER_OOO);
                }
            }
        }
        Ok(ResGuard { segs: segs.to_vec(), events: Vec::new(), ev_cap: 16, ev_total: 0 })
    }

    fn valid(&self) -> Result<(), u32> {
        for s in self.segs.iter() {
            if s.end <= s.start {
                return Err(XC_RANGE_BAD);
            }
        }
        Ok(())
    }

    /// 拦截核：请求区间 [addr, addr + frames*4K) 与保留段相交即拦。
    /// 部分跨界（起点在段外但区间相交）与整段命中同罪，标记区分：
    /// 整段命中记类别码，部分跨界记 0（跨界物证）。
    pub fn check(&mut self, addr: u32, frames: u32) -> Result<(), u32> {
        if frames == 0 {
            return Ok(()); // 零帧请求无事发生
        }
        let req_end = addr.checked_add(frames.saturating_mul(4096)).ok_or(XC_RESERVE_HIT)?;
        for s in self.segs.iter() {
            let hit = addr < s.end && s.start < req_end;
            if hit {
                let tag = if addr >= s.start { s.kind } else { 0 };
                self.record(addr, frames, tag);
                return Err(XC_RESERVE_HIT);
            }
        }
        Ok(())
    }

    /// 事件入账（定深环：满丢最旧，总数不丢）。
    fn record(&mut self, addr: u32, frames: u32, kind: u8) {
        if self.events.len() >= self.ev_cap {
            self.events.remove(0);
        }
        self.events.push((addr, frames, kind));
        self.ev_total += 1;
    }

    /// 事件账。
    pub fn events(&self) -> &[(u32, u32, u8)] {
        &self.events
    }

    /// 事件总数。
    pub fn event_total(&self) -> u64 {
        self.ev_total
    }

    /// 相交核自检：两段账不相交（守卫自身自洽）。
    pub fn segs_disjoint(&self) -> bool {
        for (i, s) in self.segs.iter().enumerate() {
            for t in self.segs[i + 1..].iter() {
                let inter = s.start < t.end && t.start < s.end;
                if inter {
                    return false;
                }
            }
        }
        true
    }
}

pub fn run_unx0370_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0370_resguard");
    // 族一：守卫构建。
    let segs = vec![
        ReserveSeg::new(0x9_FC00, 0xA_0000, K_RESERVED).unwrap_or(ReserveSeg { start: 0x9_FC00, end: 0xA_0000, kind: K_RESERVED }),
        ReserveSeg { start: 0xF_0000, end: 0x10_0000, kind: K_SMBIOS },
        ReserveSeg { start: 0x10_0000, end: 0x11_0000, kind: K_ACPI },
    ];
    let mut g = match ResGuard::new(&segs) {
        Ok(x) => x,
        Err(_) => {
            cs.add("守卫构建", false, "被拒");
            return cs;
        }
    };
    cs.add("段账自洽", g.segs_disjoint(), "三段无交");
    // 族二：整段命中拦。
    cs.add("SMBIOS 拦", g.check(0xF_0000, 1) == Err(XC_RESERVE_HIT), "命中段内");
    cs.add("事件入账", g.event_total() == 1, "首拦");
    cs.add("ACPI 拦", g.check(0x10_8000, 2).is_err(), "区间整落段内");
    // 族三：部分跨界拦。
    cs.add("尾部跨界拦", g.check(0xE_F000, 4).is_err(), "0xEF000+16K 跨入 0xF0000");
    cs.add("头部跨界拦", g.check(0x10_F000, 4).is_err(), "起于段内止于段外");
    cs.add("跨界事件区分", g.events().iter().any(|&(_, _, k)| k == 0), "跨界标记 0");
    // 族四：合法放行。
    cs.add("段间放行", g.check(0x12_0000, 4).is_ok(), "ACPI 段后");
    cs.add("低位放行", g.check(0x1000, 2).is_ok(), "IVT 区非保留账内");
    cs.add("零帧放行", g.check(0xF_0000, 0).is_ok(), "无事发生");
    // 族五：事件环。
    for i in 0..20u32 {
        let _ = g.check(0xF_0100 + i * 0x1000, 1);
    }
    cs.add("环深 16", g.events().len() == 16, "满丢最旧");
    cs.add("总数不丢", g.event_total() == 24, "4 前账 + 20 拦截");
    // 族六：构建闸。
    cs.add("空账拒", ResGuard::new(&[]).is_err(), "SEG_EMPTY");
    cs.add("倒挂拒", ReserveSeg::new(0x200, 0x100, K_ACPI).is_err(), "RANGE");
    cs.add("重叠拒", ResGuard::new(&[
        ReserveSeg { start: 0x100, end: 0x300, kind: K_ACPI },
        ReserveSeg { start: 0x200, end: 0x400, kind: K_ACPI },
    ]).is_err(), "OVERLAP");
    cs.add("乱序拒", ResGuard::new(&[
        ReserveSeg { start: 0x300, end: 0x400, kind: K_ACPI },
        ReserveSeg { start: 0x100, end: 0x200, kind: K_ACPI },
    ]).is_err(), "ORDER");
    cs.add("类别越域拒", ReserveSeg::new(0x100, 0x200, 9).is_err(), "KIND");
    cs
}
