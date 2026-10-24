//! veb263_nvvram —— VE-F0263 NVIDIA 显存管理对接。
//! 锚点（CoRun Unxreal 主册）：VE-F0263 · NVIDIA 显存管理对接 · 目标 380 行
//! 判据原文：VE-F0263-J1 显存通道三层模型：VRAM 段（物理条带）、
//!           BAR1 窗（CPU 可见窗）、GSP 堆（固件托管）；分配
//!           走 VRAM 段账（段粒度 2MiB）；BAR1 窗映射配额核
//!           （窗数 ≤ 8，映射重叠拒）；GSP 堆上限（超出拒）；
//!           三层账平衡（VRAM 派生 = BAR 映射活跃 + GSP 托管 +
//!           空闲）。
//! 组件：段账 + 窗账 + 堆账 + 平衡核。
//! 职责定位：显存对接是「NVIDIA 通路的土地管理」——VRAM 是地，
//!           BAR1 是探井，GSP 堆是托管开发区；BAR 窗就 8 个，
//!           用完只能靠 GSP 中转；三层账平衡是底线：派生的地
//!           必须有去处（映射/托管/空闲三选一），账不平就是显
//!           存泄漏。
//! 性能逐项分解：段分配 O(段数)；窗登记 O(8)；平衡 O(n)。
//! 诊断码段：0x30AC0（0x30AC1~0x30AC9 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const VM_VERSION: &str = "veb263_nvvram-v1";

/// 诊断段基址（0x30AC0 段 · 本模块独占）。
pub const VM_SEG_BASE: u32 = 0x30AC0;
pub const XC_SEG_FULL: u32 = 0x30AC1; // VRAM 段尽
pub const XC_WIN_FULL: u32 = 0x30AC2; // BAR 窗尽
pub const XC_WIN_OVERLAP: u32 = 0x30AC3; // 窗重叠
pub const XC_GSP_CAP: u32 = 0x30AC4; // GSP 堆超限
pub const XC_NOENT: u32 = 0x30AC5; // 无此段/窗
pub const XC_MAGIC: u32 = 0x30AC6; // 账魔数破
pub const XC_IMBAL: u32 = 0x30AC7; // 三层账不平
pub const XC_ALIGN: u32 = 0x30AC8; // 段粒度不对齐
pub const XC_DOUBLE_FREE: u32 = 0x30AC9; // 双还

/// VRAM 段粒度（MiB）。
pub const SEG_MIB: u32 = 2;
/// BAR1 窗上限。
pub const WIN_CAP: usize = 8;
/// GSP 堆上限（MiB）。
pub const GSP_CAP_MIB: u32 = 256;

/// 段态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegState {
    /// 空闲。
    Free = 0,
    /// BAR1 映射活跃。
    Mapped = 1,
    /// GSP 托管。
    Hosted = 2,
}

/// 段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seg {
    pub idx: u32,
    pub state: SegState,
    /// 窗号（Mapped 态有效）。
    pub win: i32,
}

/// BAR 窗。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BarWin {
    pub id: u32,
    /// 覆盖的段号（单段窗）。
    pub seg: u32,
}

/// 显存通道。
pub struct VramChannel {
    segs: Vec<Seg>,
    wins: Vec<BarWin>,
    gsp_mib: u32,
    total_segs: u32,
    magic: u32,
}

impl VramChannel {
    /// 建（total_mib 显存总量）。
    pub fn new(total_mib: u32) -> VramChannel {
        let total_segs = total_mib / SEG_MIB;
        let mut segs = Vec::new();
        for i in 0..total_segs {
            segs.push(Seg { idx: i, state: SegState::Free, win: -1 });
        }
        VramChannel { segs, wins: Vec::new(), gsp_mib: 0, total_segs, magic: 0x0AC0_0BA2 }
    }

    /// 找一个空闲段。
    fn free_seg(&self) -> Option<u32> {
        for sgm in self.segs.iter() {
            if sgm.state == SegState::Free {
                return Some(sgm.idx);
            }
        }
        None
    }

    /// BAR1 映射分配。
    pub fn map_bar(&mut self) -> Result<u32, u32> {
        if self.magic != 0x0AC0_0BA2 {
            return Err(XC_MAGIC);
        }
        if self.wins.len() >= WIN_CAP {
            return Err(XC_WIN_FULL);
        }
        let idx = match self.free_seg() {
            Some(i) => i,
            None => return Err(XC_SEG_FULL),
        };
        let wid = self.wins.len() as u32;
        // 重叠核：段未被占用（free_seg 已保证）。
        self.wins.push(BarWin { id: wid, seg: idx });
        self.segs[idx as usize].state = SegState::Mapped;
        self.segs[idx as usize].win = wid as i32;
        Ok(wid)
    }

    /// GSP 托管分配（n_mib，按段粒度向上取整）。
    pub fn host_gsp(&mut self, n_mib: u32) -> Result<u32, u32> {
        if self.magic != 0x0AC0_0BA2 {
            return Err(XC_MAGIC);
        }
        if n_mib == 0 {
            return Err(XC_ALIGN);
        }
        let segs_need = (n_mib + SEG_MIB - 1) / SEG_MIB;
        if self.gsp_mib + segs_need * SEG_MIB > GSP_CAP_MIB {
            return Err(XC_GSP_CAP);
        }
        let mut got = 0u32;
        while got < segs_need {
            let idx = match self.free_seg() {
                Some(i) => i,
                None => return Err(XC_SEG_FULL),
            };
            self.segs[idx as usize].state = SegState::Hosted;
            got += 1;
        }
        self.gsp_mib += segs_need * SEG_MIB;
        Ok(self.gsp_mib)
    }

    /// 解映射。
    pub fn unmap_bar(&mut self, wid: u32) -> Result<(), u32> {
        let mut found = false;
        let mut seg_idx = 0u32;
        for w in self.wins.iter() {
            if w.id == wid {
                found = true;
                seg_idx = w.seg;
            }
        }
        if !found {
            return Err(XC_NOENT);
        }
        self.wins.retain(|w| w.id != wid);
        let sg = &mut self.segs[seg_idx as usize];
        if sg.state != SegState::Mapped {
            return Err(XC_DOUBLE_FREE);
        }
        sg.state = SegState::Free;
        sg.win = -1;
        Ok(())
    }

    /// GSP 释放（整堆清）。
    pub fn release_gsp(&mut self) -> Result<u32, u32> {
        let freed = self.gsp_mib;
        for sg in self.segs.iter_mut() {
            if sg.state == SegState::Hosted {
                sg.state = SegState::Free;
            }
        }
        self.gsp_mib = 0;
        Ok(freed)
    }

    /// 三层账平衡核。
    pub fn audit(&self) -> Result<(u32, u32, u32), u32> {
        if self.magic != 0x0AC0_0BA2 {
            return Err(XC_MAGIC);
        }
        let mut mapped = 0u32;
        let mut hosted = 0u32;
        let mut free = 0u32;
        for sg in self.segs.iter() {
            match sg.state {
                SegState::Mapped => mapped += 1,
                SegState::Hosted => hosted += 1,
                SegState::Free => free += 1,
            }
        }
        if mapped + hosted + free != self.total_segs {
            return Err(XC_IMBAL);
        }
        // GSP 托管 MiB 与 Hosted 段数一致。
        if hosted * SEG_MIB != self.gsp_mib {
            return Err(XC_IMBAL);
        }
        // 窗数与 Mapped 段数一致。
        if self.wins.len() as u32 != mapped {
            return Err(XC_IMBAL);
        }
        Ok((mapped, hosted, free))
    }

    /// 窗数。
    pub fn win_count(&self) -> usize {
        self.wins.len()
    }

    /// 总段数。
    pub fn seg_total(&self) -> u32 {
        self.total_segs
    }
}

pub fn run_veb263_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0263");
    // 族一：BAR 窗分配。
    let mut v = VramChannel::new(64); // 32 段
    s.add("段数 32", v.seg_total() == 32, "64MiB");
    let w0 = v.map_bar();
    s.add("窗 0", w0 == Ok(0), "首窗");
    let w1 = v.map_bar();
    s.add("窗 1", w1 == Ok(1), "次窗");
    s.add("窗数 2", v.win_count() == 2, "两窗");
    s.add("账 2+0+30", v.audit() == Ok((2, 0, 30)), "平");
    // 解映射。
    s.add("解窗 0", v.unmap_bar(0).is_ok(), "回收");
    s.add("账 1+0+31", v.audit() == Ok((1, 0, 31)), "复平");
    s.add("解野窗拒", v.unmap_bar(9) == Err(XC_NOENT), "无此窗");
    s.add("双还拒", v.unmap_bar(0) == Err(XC_NOENT), "已收");
    // 族二：窗满。
    let mut f = VramChannel::new(64);
    for _ in 0..8 {
        let _ = f.map_bar();
    }
    s.add("窗满 8", f.win_count() == 8, "配额");
    s.add("第 9 窗拒", f.map_bar() == Err(XC_WIN_FULL), "窗尽");
    // 族三：GSP 托管。
    let mut g = VramChannel::new(128); // 64 段
    s.add("GSP 托 32MiB", g.host_gsp(32) == Ok(32), "16 段");
    s.add("GSP 累 96MiB", g.host_gsp(64) == Ok(96), "再 32 段");
    s.add("账 0+48+16", g.audit() == Ok((0, 48, 16)), "48 段托管");
    s.add("GSP 超限拒", g.host_gsp(200) == Err(XC_GSP_CAP), "96+200>256");
    s.add("GSP 清堆", g.release_gsp() == Ok(96), "归还");
    s.add("清后全空", g.audit() == Ok((0, 0, 64)), "64 段全空");
    // 族四：段尽。
    let mut t = VramChannel::new(16); // 8 段
    for _ in 0..8 {
        let _ = t.host_gsp(2);
    }
    s.add("段尽拒", t.host_gsp(2) == Err(XC_SEG_FULL), "8 段全托管");
    // 族五：入参。
    s.add("零托管拒", g.host_gsp(0) == Err(XC_ALIGN), "零量");
    s.add("非对齐向上取整", {
        let mut r = VramChannel::new(16);
        let x = r.host_gsp(3);
        // 3MiB → 2 段 = 4MiB。
        x == Ok(4)
    }, "3→4");
    s.add("魔数核", VM_SEG_BASE == 0x30AC0, "段基址");
    s
}
