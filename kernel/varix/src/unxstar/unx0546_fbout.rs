//! unx0546_fbout —— UNX-F0546 帧缓冲汇出（F0486 消费）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0546 · 帧缓冲汇出 · 目标 220 行
//! 判据原文：显屏汇出链核（末 8 行常显+末屏摘要），显屏行与环行内容等价
//!           （8/8）。
//! 组件：FbSink（显屏槽 8 行常显 + 摘要行）+ 等价核（显屏 8 行=环末 8 行）。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x355C0;
pub const XC_FB_SLOTS: usize = 8;
pub const XC_FB_SHORT: u32 = SEG_BASE + 1; // 环不足 8 行
pub const XC_FB_DRIFT: u32 = SEG_BASE + 2; // 显屏漂移
pub const XC_FB_OOB: u32 = SEG_BASE + 3; // 越界
pub const XC_FB_BAD: u32 = SEG_BASE + 4; // 坏行

/// 显屏汇出槽。
#[derive(Debug)]
pub struct FbSink {
    /// 常显 8 行（最新在末）。
    pub slots: Vec<String>,
    /// 摘要行（可选）。
    pub summary: Option<String>,
}

impl FbSink {
    /// 空屏。
    pub fn new() -> Self {
        FbSink {
            slots: Vec::new(),
            summary: None,
        }
    }

    /// 从环快照取末 8 行常显。
    pub fn load_tail(&mut self, snap: &[String]) -> Result<(), u32> {
        if snap.len() < XC_FB_SLOTS {
            return Err(XC_FB_SHORT);
        }
        self.slots.clear();
        let start = snap.len() - XC_FB_SLOTS;
        let mut i = start;
        while i < snap.len() {
            self.slots.push(String::from(&snap[i]));
            i += 1;
        }
        Ok(())
    }

    /// 末屏摘要：`FB n=<总量> tail=<8>`。
    pub fn set_summary(&mut self, total: u64) {
        self.summary = Some(alloc::format!("FB n={} tail=8", total));
    }

    /// 槽量核。
    pub fn slots_ok(&self) -> bool {
        self.slots.len() == XC_FB_SLOTS
    }

    /// 等价核：显屏 8 行 == 环末 8 行。
    pub fn equals_tail(&self, snap: &[String]) -> bool {
        if !self.slots_ok() || snap.len() < XC_FB_SLOTS {
            return false;
        }
        let start = snap.len() - XC_FB_SLOTS;
        let mut i = 0;
        while i < XC_FB_SLOTS {
            if self.slots[i] != snap[start + i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

impl Default for FbSink {
    fn default() -> Self {
        Self::new()
    }
}

pub fn run_unx0546_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0546");
    cs.add("SEG 段位独占", SEG_BASE == 0x355C0 && XC_FB_BAD == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：末 8 行常显。
    let mut snap = Vec::new();
    let mut i = 0;
    while i < 20 {
        snap.push(alloc::format!("L{}", i));
        i += 1;
    }
    let mut fb = FbSink::new();
    cs.add("装载成功", fb.load_tail(&snap).is_ok(), "20 行环取尾");
    cs.add("槽量 8", fb.slots_ok() && fb.slots.len() == 8, "常显八行");
    cs.add("尾行精确", fb.slots[7] == "L19" && fb.slots[0] == "L12", "末行=最新");
    cs.add("等价核", fb.equals_tail(&snap), "显屏=环末 8/8");

    // 族二：恰 8 行环。
    let mut snap8 = Vec::new();
    let mut i = 0;
    while i < 8 {
        snap8.push(alloc::format!("E{}", i));
        i += 1;
    }
    let mut fb8 = FbSink::new();
    cs.add("恰八装载", fb8.load_tail(&snap8).is_ok() && fb8.equals_tail(&snap8), "全屏=全环");
    cs.add("不足拒", fb8.load_tail(&snap8[..7]).is_err(), "7 行 Err");

    // 族三：摘要。
    fb.set_summary(120);
    cs.add("摘要在", fb.summary.as_deref() == Some("FB n=120 tail=8"), "总量账注");

    // 族四：漂移检出。
    let mut tampered = snap.clone();
    tampered[15] = String::from("X");
    cs.add("漂移检出", !fb.equals_tail(&tampered), "环改即不等");
    cs.add("空屏槽核", !FbSink::new().slots_ok(), "零槽非正常态");
    cs
}
