//! unx0275_dualchan —— UNX-F0275 双通道同写一致性（屏幕与串口对账）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0275 · 双通道一致性 · 目标 240 行
//! 判据原文：UNX-F0275-J1 屏幕与串口双通道同写：每条输出分配
//!           单调序号，双通道各记收到的最高序号，对账核要求
//!           双方序号差 ≤ 容忍窗（1 条）；落后方补拉补齐后差
//!           归零；单通道独走（差 > 窗）立案。
//! 组件：输出序号账（单调分配）+ 双通道游标（屏幕/串口各自
//!       最高收到号）+ 对账核（差窗 + 补拉 + 独走立案）。
//! 职责定位：双通道是「输出可靠性的冗余平面」——串口是崩溃后
//!   唯一活着的见证者，两通道各自记自己收到哪了，对账时差几
//!   条一目了然。补拉是修复不是掩盖：补齐后的游标必须相等，
//!   补不齐（独走）= 通道有物理故障，立案给 F0276 隔离。
//! 性能逐项分解：分配 O(1)；对账 O(1)；补拉 O(差)。
//! 诊断码段：0x20400（0x20401~0x20409 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const DC_VERSION: &str = "unx0275_dualchan-v1";

/// 诊断段基址（0x20400 段 · 本模块独占）。
pub const DC_SEG_BASE: u32 = 0x20400;
pub const XC_GAP_BIG: u32 = 0x20401; // 独走（差 > 窗）
pub const XC_CURSOR_OOO: u32 = 0x20402; // 游标倒退
pub const XC_SEQ_GAP: u32 = 0x20403; // 序号跳号
pub const XC_WINDOW_BAD: u32 = 0x20404; // 窗配置破
pub const XC_PULL_OVER: u32 = 0x20405; // 补拉过量
pub const XC_NO_OUT: u32 = 0x20406; // 无输出可对账
pub const XC_LONE_WALK: u32 = 0x20407; // 单通道全程独走
pub const XC_ORDER_BAD: u32 = 0x20408; // 补拉序破
pub const XC_SNAP_DRIFT: u32 = 0x20409; // 快照漂移

/// 独走容忍窗（条）。
pub const GAP_WINDOW: u32 = 1;

/// 双通道账。
pub struct DualChan {
    /// 已分配的最大序号。
    seq: u32,
    /// 屏幕通道收到的最高序号。
    scr: u32,
    /// 串口通道收到的最高序号。
    uart: u32,
}

impl DualChan {
    pub fn new() -> DualChan {
        DualChan { seq: 0, scr: 0, uart: 0 }
    }

    /// 分配下一条输出序号（单调 +1，无跳号）。
    pub fn alloc(&mut self) -> u32 {
        self.seq += 1;
        self.seq
    }

    /// 通道确认收到：游标只许前进（倒退 = 账面破）。
    pub fn ack(&mut self, which: u8, seq: u32) -> Result<(), u32> {
        if seq > self.seq {
            return Err(XC_SEQ_GAP); // 未分配号不可确认
        }
        match which {
            0 => {
                if seq < self.scr {
                    return Err(XC_CURSOR_OOO);
                }
                self.scr = seq;
            }
            1 => {
                if seq < self.uart {
                    return Err(XC_CURSOR_OOO);
                }
                self.uart = seq;
            }
            _ => return Err(XC_WINDOW_BAD),
        }
        Ok(())
    }

    /// 双方游标。
    pub fn cursors(&self) -> (u32, u32) {
        (self.scr, self.uart)
    }

    /// 对账核：|scr - uart| ≤ 窗 → Ok(差)；超窗 → Err(GAP_BIG)。
    pub fn reconcile(&self) -> Result<u32, u32> {
        if self.seq == 0 {
            return Err(XC_NO_OUT);
        }
        let d = if self.scr > self.uart { self.scr - self.uart } else { self.uart - self.scr };
        if d > GAP_WINDOW {
            return Err(XC_GAP_BIG);
        }
        Ok(d)
    }

    /// 补拉：落后方向前推进 n 条（不可越过分配头）。
    pub fn pull(&mut self, which: u8, n: u32) -> Result<(), u32> {
        let target = match which {
            0 => self.scr.checked_add(n).ok_or(XC_PULL_OVER)?,
            1 => self.uart.checked_add(n).ok_or(XC_PULL_OVER)?,
            _ => return Err(XC_WINDOW_BAD),
        };
        if target > self.seq {
            return Err(XC_PULL_OVER);
        }
        self.ack(which, target)
    }

    /// 独走立案核：分配超过窗且一方零进展 = 单通道全程独走。
    pub fn lone_walk(&self) -> Result<(), u32> {
        if self.seq >= 8 && (self.scr == 0 || self.uart == 0) {
            return Err(XC_LONE_WALK);
        }
        Ok(())
    }

    /// 快照（游标对 + 头）：用于跨轮对账。
    pub fn snapshot(&self) -> [u32; 3] {
        [self.seq, self.scr, self.uart]
    }

    /// 快照对账：两快照的分配头一致（头不可倒退/漂移）。
    pub fn snap_match(a: [u32; 3], b: [u32; 3]) -> Result<(), u32> {
        if a[0] != b[0] {
            return Err(XC_SNAP_DRIFT);
        }
        Ok(())
    }
}

pub fn run_unx0275_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0275_dualchan");
    // 族一：分配与确认。
    let mut dc = DualChan::new();
    let s1 = dc.alloc();
    let s2 = dc.alloc();
    let s3 = dc.alloc();
    cs.add("单调分配", s1 == 1 && s2 == 2 && s3 == 3, "1,2,3 无跳号");
    cs.add("屏确认", dc.ack(0, 3).is_ok(), "屏到 3");
    cs.add("串确认", dc.ack(1, 2).is_ok(), "串到 2");
    cs.add("游标对", dc.cursors() == (3, 2), "3/2");
    // 族二：对账。
    cs.add("差 1 过窗", dc.reconcile() == Ok(1), "窗 1 容忍");
    let s4 = dc.alloc();
    let _ = dc.ack(0, s4);
    cs.add("差 2 拦", dc.reconcile() == Err(XC_GAP_BIG), "4/2 超窗");
    // 族三：补拉。
    cs.add("串补拉 2", dc.pull(1, 2).is_ok(), "2→4");
    cs.add("补齐归零", dc.reconcile() == Ok(0), "游标相等");
    cs.add("补拉过量拒", dc.pull(0, 1).is_err(), "越过分配头");
    // 族四：游标倒退拒。
    cs.add("屏倒退拒", dc.ack(0, 1) == Err(XC_CURSOR_OOO), "游标单调");
    cs.add("未分配号拒", dc.ack(0, 99) == Err(XC_SEQ_GAP), "SEQ_GAP");
    // 族五：独走立案。
    let mut lone = DualChan::new();
    for _ in 0..8 {
        let s = lone.alloc();
        let _ = lone.ack(0, s); // 屏独走，串零确认
    }
    cs.add("独走立案", lone.lone_walk() == Err(XC_LONE_WALK), "串零进展 8 条");
    let mut half = DualChan::new();
    for _ in 0..8 {
        let s = half.alloc();
        let _ = half.ack(0, s);
        let _ = half.ack(1, s);
    }
    cs.add("双道齐过", half.lone_walk().is_ok(), "双方都有进展");
    // 族六：快照对账。
    let snap = dc.snapshot();
    cs.add("快照头", snap[0] == 4, "分配头 4");
    cs.add("快照匹配", DualChan::snap_match(snap, dc.snapshot()).is_ok(), "头一致");
    cs.add("快照漂移拒", DualChan::snap_match(snap, [9, 0, 0]) == Err(XC_SNAP_DRIFT), "头变即漂");
    cs.add("无输出拒", DualChan::new().reconcile() == Err(XC_NO_OUT), "空账");
    cs
}
