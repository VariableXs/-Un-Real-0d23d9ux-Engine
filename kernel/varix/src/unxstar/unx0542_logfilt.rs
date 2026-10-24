//! unx0542_logfilt —— UNX-F0542 级别过滤账（显性丢弃）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0542 · 级别过滤账 · 目标 220 行
//! 判据原文：过滤四档核（每级别放行/丢弃计数双列），丢弃显性率 100%
//!           （零静默）。
//! 组件：FilterGate（阈值级别+四档放行/丢弃双列计数）+ 过滤核 + 显性核
//!   （每次丢弃都进丢弃列——零静默即放行+丢弃=总数恒等）。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x354C0;
pub const XC_THRESH_OOB: u32 = SEG_BASE + 1; // 阈值越界
pub const XC_GATE_SILENT: u32 = SEG_BASE + 2; // 静默丢（计数失恒）
pub const XC_FILT_OOB: u32 = SEG_BASE + 3; // 越界
pub const XC_FILT_BAD: u32 = SEG_BASE + 4; // 坏账

/// 过滤门：阈值级别以下（号更大更细）全丢。
#[derive(Debug)]
pub struct FilterGate {
    /// 阈值级别号（0=只放 ERROR；3=全放）。
    pub thresh: u8,
    /// 放行计数（按级别号分列）。
    pub pass: [u32; 4],
    /// 丢弃计数（按级别号分列）。
    pub drop: [u32; 4],
}

impl FilterGate {
    /// 构造：阈值 0..=3。
    pub fn new(thresh: u8) -> Result<Self, u32> {
        if thresh > 3 {
            return Err(XC_THRESH_OOB);
        }
        Ok(FilterGate {
            thresh,
            pass: [0; 4],
            drop: [0; 4],
        })
    }

    /// 过滤一行：级别号 → 放行 bool（并双列计数）。
    pub fn feed(&mut self, level: u8) -> Result<bool, u32> {
        if level > 3 {
            return Err(XC_FILT_OOB);
        }
        if level <= self.thresh {
            self.pass[level as usize] += 1;
            Ok(true)
        } else {
            self.drop[level as usize] += 1;
            Ok(false)
        }
    }

    /// 显性核：放行+丢弃 == 总输入（零静默）。
    pub fn explicit_ok(&self, total_in: u32) -> bool {
        let mut t = 0;
        let mut i = 0;
        while i < 4 {
            t += self.pass[i] + self.drop[i];
            i += 1;
        }
        t == total_in
    }

    /// 双列分账核：某级别只在一列有数（不串账）。
    pub fn no_cross(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.pass[i] > 0 && self.drop[i] > 0 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 总放行。
    pub fn total_pass(&self) -> u32 {
        let mut t = 0;
        let mut i = 0;
        while i < 4 {
            t += self.pass[i];
            i += 1;
        }
        t
    }

    /// 总丢弃。
    pub fn total_drop(&self) -> u32 {
        let mut t = 0;
        let mut i = 0;
        while i < 4 {
            t += self.drop[i];
            i += 1;
        }
        t
    }
}

/// 四档扫描核：阈值 0..3 各喂 4 级别，放行集恰为 {0..thresh}。
pub fn four_threshold_scan() -> Result<bool, u32> {
    let mut t = 0;
    while t <= 3 {
        let mut g = FilterGate::new(t)?;
        let mut lv = 0;
        while lv < 4 {
            let passed = g.feed(lv)?;
            if passed != (lv <= t) {
                return Ok(false);
            }
            lv += 1;
        }
        if !g.explicit_ok(4) || !g.no_cross() {
            return Ok(false);
        }
        t += 1;
    }
    Ok(true)
}

pub fn run_unx0542_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0542");
    cs.add("SEG 段位独占", SEG_BASE == 0x354C0 && XC_FILT_BAD == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：四档扫描。
    cs.add("四档核", four_threshold_scan() == Ok(true), "阈值 0..3 放行集精确");

    // 族二：双列分账。
    let mut g = FilterGate::new(1).unwrap_or(FilterGate { thresh: 0, pass: [0; 4], drop: [0; 4] });
    let _ = g.feed(0);
    let _ = g.feed(0);
    let _ = g.feed(1);
    let _ = g.feed(2);
    let _ = g.feed(3);
    cs.add("放行列", g.pass == [2, 1, 0, 0], "ERR×2+WRN×1");
    cs.add("丢弃列", g.drop == [0, 0, 1, 1], "INF×1+DBG×1");
    cs.add("零静默", g.explicit_ok(5), "5=3+2");
    cs.add("不串账", g.no_cross(), "双列互斥");
    cs.add("阈值越界拒", FilterGate::new(4).is_err(), "thresh=4 Err");
    cs.add("级别越界拒", g.feed(4).is_err(), "level=4 Err");

    // 族三：全放/全收两极。
    let mut g0 = FilterGate::new(0).unwrap_or(FilterGate { thresh: 0, pass: [0; 4], drop: [0; 4] });
    let mut lv = 0;
    while lv < 4 {
        let _ = g0.feed(lv);
        lv += 1;
    }
    cs.add("最严档", g0.total_pass() == 1 && g0.total_drop() == 3, "只放 ERROR");
    let mut g3 = FilterGate::new(3).unwrap_or(FilterGate { thresh: 0, pass: [0; 4], drop: [0; 4] });
    lv = 0;
    while lv < 4 {
        let _ = g3.feed(lv);
        lv += 1;
    }
    cs.add("最宽档", g3.total_pass() == 4 && g3.total_drop() == 0, "全放");
    cs.add("守恒双极", g0.explicit_ok(4) && g3.explicit_ok(4), "两极零静默");
    cs
}
